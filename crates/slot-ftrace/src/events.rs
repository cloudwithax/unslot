//! Every event id and name. Spans are begin/end pairs (the end carries the same arg unless the
//! note says otherwise), instants mark a moment, counters carry a value in `arg`.

macro_rules! events {
    ($($(#[$doc:meta])* $id:literal $name:ident $text:literal;)*) => {
        pub mod ev {
            $($(#[$doc])* pub const $name: u16 = $id;)*
        }
        /// Id and name of every event, as written to `<path>.names`.
        pub const NAMES: &[(u16, &str)] = &[$(($id, $text)),*];
    };
}

events! {
    // The emu thread, "slot-emu".
    /// One pass of the emu loop; arg is the pass number.
    100 EMU_ITER "emu.iter";
    /// A Cmd applied (end carries the same kind): 0 Load, 1 Save, 2 Sav, 3 Thumb, 4 BeginLink,
    /// 5 EndLink, 6 BeginCable, 7 SetOption, 8 Options, 9 SetDisk, 10 Refresh, 11 Cheevos.
    101 EMU_APPLY "emu.apply";
    /// Starting the achievements tracker.
    102 EMU_CHEEVOS_START "emu.cheevos_start";
    /// Taking the tracker's notices.
    103 EMU_TRACKER_TAKE "emu.tracker_take";
    /// Link transport, cable and core.pump_link work before the frame.
    104 EMU_LINK "emu.link";
    /// Counter: 0 paused, 1 normal, 2 fast.
    105 EMU_SPEED "emu.speed";
    /// Counter: 1 while the emu is locked to the presenter's ticks.
    106 EMU_LOCK "emu.lock";
    /// The pad as the emu read it, as a bit mask.
    107 EMU_INPUT "emu.input";
    /// Rewinding one step.
    108 EMU_REWIND "emu.rewind";
    /// Frame skip decision for the frame about to run: 1 skip it, 0 draw it.
    109 EMU_SKIP "emu.skip";
    /// A core frame is about to start; arg is its index within this present.
    110 EMU_FRAME_BEGAN "emu.frame_began";
    /// Waiting for the cable peer.
    111 EMU_CABLE_WAIT "emu.cable_wait";
    /// core.run_frame_linked.
    112 EMU_RUN_LINKED "emu.run_linked";
    /// core.run_frame_overlapped (the remote core only sends the request here).
    113 EMU_RUN_OVERLAPPED "emu.run_overlapped";
    /// core.run_frame.
    114 EMU_RUN_FRAME "emu.run_frame";
    /// Run-ahead's run; arg is the frames ahead.
    115 EMU_RUN_AHEAD "emu.run_ahead";
    /// The achievements tracker's per-frame work (t.frame).
    116 EMU_TRACKER_FRAME "emu.tracker_frame";
    /// Counter: ns one core frame took, frame_began to done.
    117 EMU_FRAME_NS "emu.frame_ns";
    /// Counter: ns the whole run loop for this present took.
    118 EMU_CORE_NS "emu.core_ns";
    /// Counter: core frames run this present.
    119 EMU_RAN "emu.ran";
    /// Counter: present periods this run covers.
    120 EMU_SPAN "emu.span";
    /// Counter: cost.measured's skipped-frame time in ns, -1 when none was skipped.
    121 EMU_COST_SKIPPED_NS "emu.cost_skipped_ns";
    /// Counter: cost.measured's drawn-frame time in ns.
    122 EMU_COST_DRAWN_NS "emu.cost_drawn_ns";
    /// Counter: FrameCost's estimate of a drawn frame, ns, after measured.
    123 EMU_COST_DRAW_NS "emu.cost_draw_ns";
    /// Copying the picture into Frames and publishing it; begin arg bytes, end arg the
    /// publish sequence number (matches render.frame).
    124 EMU_PUBLISH "emu.publish";
    /// frame_done: instant, arg is the tick served.
    125 EMU_FRAME_DONE "emu.frame_done";
    /// core.serialize for a rewind snapshot; end arg bytes, -1 on error.
    126 EMU_SNAPSHOT "emu.snapshot";
    /// Handing the snapshot to the rewind thread.
    127 EMU_REWIND_PUSH "emu.rewind_push";
    /// core.take_audio; end arg is frames.
    128 EMU_TAKE_AUDIO "emu.take_audio";
    /// Counter: audio frames queued in the ring when the emu looked.
    129 EMU_RING_QUEUED "emu.ring_queued";
    /// Counter: the ring's target fill in frames.
    130 EMU_RING_TARGET "emu.ring_target";
    /// Counter: ns since the emu's last audio push.
    131 EMU_DT_NS "emu.dt_ns";
    /// Counter: the drift trim, parts per million off 1.
    132 EMU_TRIM_PPM "emu.trim_ppm";
    /// Counter: the whole resample ratio, parts per million off 1.
    133 EMU_RATIO_PPM "emu.ratio_ppm";
    /// Resampling and volume; begin arg frames in, end arg frames out.
    134 EMU_RESAMPLE "emu.resample";
    /// ring.push_blocking, which can block; begin arg frames, end arg frames now queued.
    135 EMU_RING_PUSH "emu.ring_push";
    /// Counter: ns the emu spent after the core, for fast-forward.
    136 EMU_POST_COST_NS "emu.post_cost_ns";
    /// Waiting for the next present tick; begin arg the tick served, end arg the tick that came,
    /// -1 a request woke it, -2 it ran late.
    137 EMU_TICK_WAIT "emu.tick_wait";
    /// Counter: ticks advanced since the last one served (1 is on time, more are missed).
    138 EMU_TICK_GAP "emu.tick_gap";
    /// Counter: the measured tick rate against 60 Hz, parts per million off 1.
    139 EMU_SCALE_PPM "emu.scale_ppm";
    /// A command woke the tick wait; arg the tick served.
    140 EMU_WAIT_REQUEST "emu.wait_request";
    /// The tick wait ran out (Wait::Late); arg the tick served.
    141 EMU_WAIT_LATE "emu.wait_late";
    /// Sleeping to the deadline when not locked; arg the ns.
    142 EMU_SLEEP "emu.sleep";
    /// The deadline had passed already; arg is how late in ns.
    143 EMU_DEADLINE_MISS "emu.deadline_miss";
    /// Tracker idle when nothing ran.
    144 EMU_TRACKER_IDLE "emu.tracker_idle";
    /// A frame replaced one the presenter never took; arg is the lost frame's sequence number.
    145 FRAMES_OVERWRITTEN "frames.overwritten";

    // The present thread (slot's main thread).
    /// One pass of the present loop; arg is the pass number.
    200 PRESENT_ITER "present.iter";
    /// Pacer sleep before the pass; arg is the pacer's period in ns.
    201 PRESENT_PACER_WAIT "present.pacer_wait";
    /// Frontend::advance: input, UI state.
    202 PRESENT_ADVANCE "present.advance";
    /// Sending the tick and waiting for the emu to finish its frame (step_emulator).
    203 PRESENT_STEP "present.step";
    /// Waiting for the emu's frame_done; end arg 1 done, 0 timed out.
    204 PRESENT_WAIT_FRAME "present.wait_frame";
    /// Frontend::render, GL work to the offscreen target and the panel buffer.
    205 PRESENT_RENDER "present.render";
    /// eglSwapBuffers (Surface::swap), which blocks on the page flip; end arg ns.
    206 PRESENT_SWAP "present.swap";
    /// The min-frame sleep after the swap; arg ns.
    207 PRESENT_MIN_SLEEP "present.min_sleep";
    /// Counter: ns from loop pass start to just before the swap.
    208 PRESENT_WORK_NS "present.work_ns";
    /// Counter: ns since the previous swap returned.
    209 PRESENT_SEEN_NS "present.seen_ns";
    /// Counter: the pacer's current period estimate in ns.
    210 PRESENT_PERIOD_NS "present.period_ns";
    /// Counter: the pacer's worst recent work time in ns.
    211 PRESENT_WORK_MAX_NS "present.work_max_ns";
    /// A swap returned more than 1.5 periods after the last; arg ns.
    212 PRESENT_MISSED "present.missed";
    /// The vblank/present tick sent to the emu; arg is its number. The tick source is the
    /// present loop itself, paced by the pacer and the swap.
    213 TICK "tick";
    /// Counter: the present period sent with the tick, ns.
    214 TICK_PRESENT_NS "tick.present_ns";
    /// Rumble set; arg strength.
    215 RUMBLE_SET "rumble.set";
    /// Input poll.
    216 INPUT_POLL "input.poll";
    /// Feeding events to gestures and the pad; arg event count.
    217 INPUT_FEED "input.feed";
    /// Session::update (app, emu syncing, rumble, prefetch).
    218 SESSION_UPDATE "session.update";
    /// App::update inside it.
    219 SESSION_APP_UPDATE "session.app_update";
    /// Frontend::compose.
    220 RENDER_COMPOSE "render.compose";
    /// A published frame was picked up; arg is its sequence number (matches emu.publish end).
    221 RENDER_FRAME "render.frame";
    /// No new frame: the previous picture is drawn again.
    222 RENDER_REPEAT "render.repeat";
    /// Uploading the game texture; arg the frame's sequence number.
    223 GFX_UPLOAD_GAME "gfx.upload_game";
    /// compositor.begin_frame (bind, clear).
    224 GFX_BEGIN_FRAME "gfx.begin_frame";
    /// The texture and sprite syncs between begin_frame and the draw list.
    225 RENDER_SYNC "render.sync";
    /// App::draw building the draw list (menus, HUD, overlays).
    226 RENDER_APP_DRAW "render.app_draw";
    /// compositor.draw_list; arg is the item count. The game, HUD and FPS counter draw here.
    227 GFX_DRAW_LIST "gfx.draw_list";
    /// compositor.end_frame (final pass to the panel).
    228 GFX_END_FRAME "gfx.end_frame";
    /// Counter: the on-screen FPS counter's new value times 1000. The counter counts frames the
    /// emu published per wall second over 500 ms windows, not presents.
    229 UI_FPS_MILLI "ui.fps_milli";
    /// Counter: published frames in that window.
    230 UI_FPS_FRAMES "ui.fps_frames";
    /// Counter: that window's length in ns.
    231 UI_FPS_WINDOW_NS "ui.fps_window_ns";

    // The audio output thread, "slot-audio" (ALSA) or the cpal callback thread.
    /// ring.fill, taking a period from the ring.
    300 AUDIO_FILL "audio.fill";
    /// snd_pcm_writei for one period, which blocks on the device; begin arg frames, end arg 1
    /// if it wrote.
    301 AUDIO_WRITE "audio.write";
    /// Counter: frames queued in the ALSA device itself (snd_pcm_delay) after a write.
    302 AUDIO_DEVICE_DELAY "audio.device_delay";
    /// snd_pcm_writei failed (an xrun); arg the error.
    303 AUDIO_XRUN "audio.xrun";
    /// The thread slept because no device was open.
    304 AUDIO_NO_PCM_SLEEP "audio.no_pcm_sleep";
    /// A cpal output callback; arg frames.
    305 AUDIO_CALLBACK "audio.callback";
    /// Counter: ring frames queued after a fill.
    306 RING_QUEUED "ring.queued";
    /// Counter: 1 while the ring is re-priming and playing silence.
    307 RING_HOLDING "ring.holding";
    /// The output thread found the ring short; arg samples short.
    308 RING_UNDERRUN "ring.underrun";
    /// Counter: samples short in all, since the ring was opened.
    309 RING_UNDERRUNS "ring.underruns";
    /// The ring was full and dropped samples; arg samples dropped.
    310 RING_OVERRUN "ring.overrun";
    /// Counter: samples dropped in all.
    311 RING_OVERRUNS "ring.overruns";
    /// push_blocking waited for room in the ring; begin arg frames left to write, end arg
    /// frames then queued.
    312 RING_PUSH_WAIT "ring.push_wait";
    /// push_blocking gave up after its longest wait; arg samples dropped.
    313 RING_PUSH_TIMEOUT "ring.push_timeout";

    // The libretro wrapper: in the core host process on the device for GPU cores, otherwise in
    // slot's emu thread. The core's own threads call the audio callbacks.
    /// retro_run.
    400 RETRO_RUN "retro.run";
    /// The video_refresh callback; arg is width<<32|height.
    401 RETRO_VIDEO_REFRESH "retro.video_refresh";
    /// video_refresh called with no picture (a duplicate frame).
    402 RETRO_VIDEO_EMPTY "retro.video_empty";
    /// The audio_batch callback; arg frames.
    403 RETRO_AUDIO_BATCH "retro.audio_batch";
    /// Counter: frames of core-thread sound waiting for slot, seen at a batch.
    404 RETRO_AUDIO_BACKLOG "retro.audio_backlog";
    /// A core thread waiting for slot to take sound; end arg 1 if it timed out.
    405 RETRO_AUDIO_WAIT "retro.audio_wait";
    /// The core's input_poll callback.
    406 RETRO_INPUT_POLL "retro.input_poll";
    /// slot taking the core-thread sound; arg is the frames it already held from the host's own.
    407 RETRO_AUDIO_TAKE "retro.audio_take";

    // The core host protocol. REMOTE_* run in slot's emu thread, HOST_* in the core host process.
    /// A request and its reply; arg the call (1 is RUN).
    500 REMOTE_CALL "remote.call";
    /// Reading the reply to an overlapped run: the wait for the core's frame.
    501 REMOTE_COLLECT "remote.collect";
    /// Sending an overlapped run, settling the one before.
    502 REMOTE_RUN_OVERLAPPED "remote.run_overlapped";
    /// Writing the request to the socket; arg bytes.
    503 REMOTE_SEND "remote.send";
    /// Taking up a reply's state, audio and picture; arg bytes.
    504 REMOTE_TAKE_STATE "remote.take_state";
    /// The host waiting for a request; end arg is the call.
    510 HOST_WAIT_REQUEST "host.wait_request";
    /// The host running the call.
    511 HOST_ANSWER "host.answer";
    /// Building the reply's state, picture and audio; end arg bytes.
    512 HOST_PUT_STATE "host.put_state";
    /// Sending the reply; begin arg bytes.
    513 HOST_SEND "host.send";
}
