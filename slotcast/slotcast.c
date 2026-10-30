/*
 * libslotcast: the encoders unslot casts with, x264 for the picture and Opus or AAC for the sound, and
 * mbedTLS for talking to the TV, which the handheld has no library for, behind a few calls of its
 * own, so slot loads them at run time with nothing of their structs to keep in step with.
 */
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include <faac.h>
#include <mbedtls/ctr_drbg.h>
#include <mbedtls/entropy.h>
#include <mbedtls/net_sockets.h>
#include <mbedtls/ssl.h>
#include <opus.h>
#include <psa/crypto.h>
#include <x264.h>

#define EXPORT __attribute__((visibility("default")))

struct h264 {
	x264_t *x;
	x264_picture_t in;
	int w, h;
	int64_t pts;
};

/* An H.264 encoder for w by h I420 frames at fps a second, kbps kilobits a second: Constrained
 * Baseline, one frame in for one out, each frame cut in slices encoded on threads threads at
 * once, no frame after the first key frame a key frame unless asked for, SPS and PPS before each
 * one, Annex B. NULL when x264 will not have it. */
EXPORT void *slotcast_h264_open(int w, int h, int fps, int kbps, int threads)
{
	x264_param_t p;
	if (x264_param_default_preset(&p, "ultrafast", "zerolatency") < 0)
		return NULL;
	p.i_width = w;
	p.i_height = h;
	p.i_csp = X264_CSP_I420;
	p.i_fps_num = fps;
	p.i_fps_den = 1;
	p.i_timebase_num = 1;
	p.i_timebase_den = fps;
	p.i_threads = threads;
	p.b_sliced_threads = threads > 1;
	p.i_log_level = X264_LOG_WARNING;
	p.i_keyint_max = X264_KEYINT_MAX_INFINITE;
	p.rc.i_rc_method = X264_RC_ABR;
	p.rc.i_bitrate = kbps;
	p.rc.i_vbv_max_bitrate = kbps;
	/* An eighth of a second of buffer: a key frame fits without the frames after it starving,
	 * and no frame is so big it is late to the TV. */
	p.rc.i_vbv_buffer_size = kbps / 8;
	p.b_repeat_headers = 1;
	p.b_annexb = 1;
	p.vui.b_fullrange = 0;
	if (x264_param_apply_profile(&p, "baseline") < 0)
		return NULL;
	struct h264 *e = calloc(1, sizeof *e);
	if (!e)
		return NULL;
	e->x = x264_encoder_open(&p);
	if (!e->x) {
		free(e);
		return NULL;
	}
	x264_picture_init(&e->in);
	e->in.img.i_csp = X264_CSP_I420;
	e->in.img.i_plane = 3;
	e->in.img.i_stride[0] = w;
	e->in.img.i_stride[1] = w / 2;
	e->in.img.i_stride[2] = w / 2;
	e->w = w;
	e->h = h;
	return e;
}

/* Encodes one I420 frame, its planes one after another, a key frame when key_wanted. The frame's
 * NAL units, back to back, are at *out until the next call; *key says whether it is a key frame.
 * Returns their length, or < 0 on an error. */
EXPORT int slotcast_h264_encode(void *enc, const uint8_t *i420, int key_wanted,
				const uint8_t **out, int *key)
{
	struct h264 *e = enc;
	size_t luma = (size_t)e->w * e->h;
	e->in.img.plane[0] = (uint8_t *)i420;
	e->in.img.plane[1] = (uint8_t *)i420 + luma;
	e->in.img.plane[2] = (uint8_t *)i420 + luma + luma / 4;
	e->in.i_pts = e->pts++;
	e->in.i_type = key_wanted ? X264_TYPE_IDR : X264_TYPE_AUTO;
	x264_nal_t *nal;
	int n;
	x264_picture_t pic;
	int len = x264_encoder_encode(e->x, &nal, &n, &e->in, &pic);
	if (len <= 0) {
		*out = NULL;
		*key = 0;
		return len;
	}
	/* x264 lays a frame's NAL units out one after another. */
	*out = nal[0].p_payload;
	*key = pic.b_keyframe;
	return len;
}

EXPORT void slotcast_h264_close(void *enc)
{
	struct h264 *e = enc;
	if (!e)
		return;
	x264_encoder_close(e->x);
	free(e);
}

/* An Opus encoder for 16-bit interleaved sound at rate and channels, bps bits a second, tuned
 * for delay over quality. NULL when Opus will not have it. */
EXPORT void *slotcast_opus_open(int rate, int channels, int bps)
{
	int err;
	OpusEncoder *e =
		opus_encoder_create(rate, channels, OPUS_APPLICATION_RESTRICTED_LOWDELAY, &err);
	if (err != OPUS_OK)
		return NULL;
	opus_encoder_ctl(e, OPUS_SET_BITRATE(bps));
	return e;
}

/* Encodes frames frames of sound, 2.5 to 60 ms of it, into out. Returns the packet's length, or
 * < 0 on an error. */
EXPORT int slotcast_opus_encode(void *enc, const int16_t *pcm, int frames, uint8_t *out, int cap)
{
	return opus_encode(enc, pcm, frames, out, cap);
}

EXPORT void slotcast_opus_close(void *enc)
{
	if (enc)
		opus_encoder_destroy(enc);
}

/* An AAC-LC encoder for 16-bit interleaved stereo at rate, bps bits a second, each frame in ADTS.
 * NULL when FAAC will not have it. */
EXPORT void *slotcast_aac_open(int rate, int bps)
{
	faac_params p;
	faac_encoder *e = NULL;
	if (faac_params_init(&p, sizeof(p)) != FAAC_OK)
		return NULL;
	p.sample_rate = rate;
	p.num_channels = 2;
	p.object_type = FAAC_OBJ_LOW;
	p.bit_rate = bps / 2;
	p.use_tns = true;
	p.output_format = FAAC_STREAM_ADTS;
	p.input_format = FAAC_INPUT_16BIT;
	if (faac_encoder_open(&p, &e) != FAAC_OK)
		return NULL;
	return e;
}

/* Takes frames stereo frames of sound, 1024 at most, and writes a frame of AAC into out, none
 * the first few calls while it fills. Returns the frame's length, 0 for none, or < 0 on an
 * error. */
EXPORT int slotcast_aac_encode(void *enc, const int16_t *pcm, int frames, uint8_t *out, int cap)
{
	uint32_t n = 0;
	if (faac_encoder_encode(enc, pcm, frames * 2, out, cap, &n) != FAAC_OK)
		return -1;
	return n;
}

EXPORT void slotcast_aac_close(void *enc)
{
	faac_encoder_close((faac_encoder **)&enc);
}

struct tls {
	mbedtls_net_context net;
	mbedtls_entropy_context entropy;
	mbedtls_ctr_drbg_context drbg;
	mbedtls_ssl_config conf;
	mbedtls_ssl_context ssl;
};

EXPORT void slotcast_tls_close(void *t)
{
	struct tls *s = t;
	mbedtls_ssl_close_notify(&s->ssl);
	mbedtls_ssl_free(&s->ssl);
	mbedtls_ssl_config_free(&s->conf);
	mbedtls_ctr_drbg_free(&s->drbg);
	mbedtls_entropy_free(&s->entropy);
	free(s);
}

/* TLS as a client over fd, a connected TCP socket the caller keeps and closes. A Chromecast's
 * certificate is signed by nobody a client would know, so it is not checked. NULL when the
 * handshake fails. */
EXPORT void *slotcast_tls_connect(int fd)
{
	struct tls *s = calloc(1, sizeof *s);
	if (!s || psa_crypto_init() != PSA_SUCCESS) {
		free(s);
		return NULL;
	}
	s->net.fd = fd;
	mbedtls_entropy_init(&s->entropy);
	mbedtls_ctr_drbg_init(&s->drbg);
	mbedtls_ssl_config_init(&s->conf);
	mbedtls_ssl_init(&s->ssl);
	int ret;
	if (mbedtls_ctr_drbg_seed(&s->drbg, mbedtls_entropy_func, &s->entropy, NULL, 0) ||
	    mbedtls_ssl_config_defaults(&s->conf, MBEDTLS_SSL_IS_CLIENT,
					MBEDTLS_SSL_TRANSPORT_STREAM, MBEDTLS_SSL_PRESET_DEFAULT))
		goto fail;
	mbedtls_ssl_conf_authmode(&s->conf, MBEDTLS_SSL_VERIFY_NONE);
	mbedtls_ssl_conf_rng(&s->conf, mbedtls_ctr_drbg_random, &s->drbg);
	if (mbedtls_ssl_setup(&s->ssl, &s->conf))
		goto fail;
	mbedtls_ssl_set_bio(&s->ssl, &s->net, mbedtls_net_send, mbedtls_net_recv, NULL);
	while ((ret = mbedtls_ssl_handshake(&s->ssl)) != 0)
		if (ret != MBEDTLS_ERR_SSL_WANT_READ && ret != MBEDTLS_ERR_SSL_WANT_WRITE)
			goto fail;
	return s;
fail:
	mbedtls_ssl_free(&s->ssl);
	mbedtls_ssl_config_free(&s->conf);
	mbedtls_ctr_drbg_free(&s->drbg);
	mbedtls_entropy_free(&s->entropy);
	free(s);
	return NULL;
}

/* Up to len bytes read into buf: how many, or 0 or less when the connection is over. TLS 1.3's
 * tickets, which come after the handshake, are read past. */
EXPORT int slotcast_tls_read(void *t, unsigned char *buf, int len)
{
	struct tls *s = t;
	for (;;) {
		int n = mbedtls_ssl_read(&s->ssl, buf, len);
		if (n != MBEDTLS_ERR_SSL_RECEIVED_NEW_SESSION_TICKET &&
		    n != MBEDTLS_ERR_SSL_WANT_READ && n != MBEDTLS_ERR_SSL_WANT_WRITE)
			return n;
	}
}

/* Up to len bytes of buf written: how many, or 0 or less when the connection is over. */
EXPORT int slotcast_tls_write(void *t, const unsigned char *buf, int len)
{
	struct tls *s = t;
	for (;;) {
		int n = mbedtls_ssl_write(&s->ssl, buf, len);
		if (n != MBEDTLS_ERR_SSL_WANT_READ && n != MBEDTLS_ERR_SSL_WANT_WRITE)
			return n;
	}
}

/* How many bytes are read already and waiting. */
EXPORT int slotcast_tls_pending(void *t)
{
	return (int)mbedtls_ssl_get_bytes_avail(&((struct tls *)t)->ssl);
}
