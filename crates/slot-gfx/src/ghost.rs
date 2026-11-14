use crate::quad::Quad;

/// How each game frame is blended with the ones before it.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum FrameBlend {
    #[default]
    Off,
    /// Each frame averaged with the last.
    Mix,
    /// An LCD's slow response, rising and falling at the panel's own rates.
    Ghosting,
}

/// Whose LCD the ghosting follows.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Lcd {
    Dmg,
    Gbc,
    #[default]
    Gba,
}

impl Lcd {
    /// How much of the last frame a pixel keeps while it brightens, and while it darkens.
    pub fn response(self) -> [f32; 2] {
        match self {
            Lcd::Dmg => [keep(61.0), keep(21.0)],
            Lcd::Gbc => [keep(16.0), keep(8.0)],
            Lcd::Gba => [keep(20.0), keep(10.0)],
        }
    }
}

/// What a pixel keeps of the frame before at ~60 fps, for a response time of `ms`.
fn keep(ms: f32) -> f32 {
    (-1000.0 / 59.73 / ms).exp()
}

/// The blended picture, ping-ponged between two textures the size of the game's.
pub(crate) struct Ghost {
    prog: gl::types::GLuint,
    u_keep: gl::types::GLint,
    tex: [gl::types::GLuint; 2],
    fbo: [gl::types::GLuint; 2],
    size: (u32, u32),
    shown: usize,
    primed: bool,
}

impl Ghost {
    pub fn new((w, h): (u32, u32)) -> Result<Self, crate::surface::GfxError> {
        let prog = crate::shaders::program(crate::shaders::BLIT_VERT, crate::shaders::GHOST_FRAG)?;
        let mut fbo = [0; 2];
        unsafe {
            gl::GenFramebuffers(2, fbo.as_mut_ptr());
            gl::UseProgram(prog);
            gl::Uniform1i(crate::gl::uniform_location(prog, "u_tex"), 0);
            gl::Uniform1i(crate::gl::uniform_location(prog, "u_prev"), 1);
            gl::Uniform4f(
                crate::gl::uniform_location(prog, "u_uv"),
                0.0,
                0.0,
                1.0,
                1.0,
            );
        }
        let u_keep = crate::gl::uniform_location(prog, "u_keep");
        let mut ghost = Ghost {
            prog,
            u_keep,
            tex: [0; 2],
            fbo,
            size: (0, 0),
            shown: 0,
            primed: false,
        };
        ghost.resize((w, h));
        Ok(ghost)
    }

    /// Makes the textures the game's size, starting the blend over.
    pub fn resize(&mut self, (w, h): (u32, u32)) {
        if self.size == (w, h) {
            return;
        }
        unsafe {
            gl::DeleteTextures(2, self.tex.as_ptr());
        }
        self.tex = [0, 1]
            .map(|_| crate::gl::texture(w, h, gl::NEAREST, gl::CLAMP_TO_EDGE, gl::RGBA, None));
        unsafe {
            let mut fb = 0;
            gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut fb);
            for (f, t) in self.fbo.iter().zip(self.tex) {
                gl::BindFramebuffer(gl::FRAMEBUFFER, *f);
                gl::FramebufferTexture2D(
                    gl::FRAMEBUFFER,
                    gl::COLOR_ATTACHMENT0,
                    gl::TEXTURE_2D,
                    t,
                    0,
                );
            }
            gl::BindFramebuffer(gl::FRAMEBUFFER, fb as gl::types::GLuint);
        }
        self.size = (w, h);
        self.primed = false;
    }

    pub fn reset(&mut self) {
        self.primed = false;
    }

    /// Blends `game` over the picture so far, keeping `keep` of it as pixels rise and fall.
    pub fn respond(&mut self, game: gl::types::GLuint, keep: [f32; 2], quad: &Quad) {
        self.draw(game, self.tex[self.shown], keep, quad);
    }

    /// Averages `game` with the frame before it.
    pub fn mix(&mut self, game: gl::types::GLuint, last: gl::types::GLuint, quad: &Quad) {
        self.draw(game, last, [0.5; 2], quad);
    }

    fn draw(
        &mut self,
        game: gl::types::GLuint,
        prev: gl::types::GLuint,
        keep: [f32; 2],
        quad: &Quad,
    ) {
        let next = 1 - self.shown;
        let keep = if self.primed { keep } else { [0.0; 2] };
        unsafe {
            let mut fb = 0;
            let mut view = [0; 4];
            gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut fb);
            gl::GetIntegerv(gl::VIEWPORT, view.as_mut_ptr());
            gl::BindFramebuffer(gl::FRAMEBUFFER, self.fbo[next]);
            gl::Viewport(0, 0, self.size.0 as i32, self.size.1 as i32);
            gl::UseProgram(self.prog);
            gl::Uniform2f(self.u_keep, keep[0], keep[1]);
            gl::ActiveTexture(gl::TEXTURE1);
            gl::BindTexture(gl::TEXTURE_2D, prev);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, game);
            quad.draw();
            gl::BindFramebuffer(gl::FRAMEBUFFER, fb as gl::types::GLuint);
            gl::Viewport(view[0], view[1], view[2], view[3]);
        }
        self.shown = next;
        self.primed = true;
    }

    /// The blended picture, once there is one.
    pub fn texture(&self) -> Option<gl::types::GLuint> {
        self.primed.then_some(self.tex[self.shown])
    }
}

impl Drop for Ghost {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteFramebuffers(2, self.fbo.as_ptr());
            gl::DeleteTextures(2, self.tex.as_ptr());
            gl::DeleteProgram(self.prog);
        }
    }
}
