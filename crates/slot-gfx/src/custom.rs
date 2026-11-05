//! A single-pass RetroArch GLSL shader the game can be drawn with in place of the built-in effects.
//! The one file holds both stages, each behind `VERTEX` or `FRAGMENT`.

use crate::surface::{GfxError, OUT_H, OUT_W};
use std::cell::Cell;

const VERTEX_AT: gl::types::GLuint = 0;
const TEX_AT: gl::types::GLuint = 1;

pub struct CustomPass {
    prog: gl::types::GLuint,
    vao: gl::types::GLuint,
    vbo: gl::types::GLuint,
    linear: bool,
    frames: Cell<u32>,
    u_mvp: gl::types::GLint,
    u_texture_size: gl::types::GLint,
    u_input_size: gl::types::GLint,
    u_output_size: gl::types::GLint,
    u_frame_count: gl::types::GLint,
    u_frame_direction: gl::types::GLint,
}

/// Where the pass draws and what it reads, all in the units the game pass already works in.
pub struct Placed {
    /// x, y, w, h on the OUT_W by OUT_H screen.
    pub rect: (f32, f32, f32, f32),
    /// The part of the texture shown, as x, y, w, h in 0..1.
    pub src: [f32; 4],
    pub texels: (u32, u32),
    /// Quarter turns counter-clockwise.
    pub rotation: u32,
    pub fbo_scale: [f32; 2],
}

/// The shader source for one stage: any `#version` the file names is replaced by the one this GL
/// takes, then the stage is defined.
pub fn stage(src: &str, stage: &str, es: bool) -> String {
    let version = match es {
        true => "#version 100\n",
        false => "#version 330 core\n",
    };
    let body: String = src
        .lines()
        .map(|l| {
            if l.trim_start().starts_with("#version") || parameter(l).is_some() {
                ""
            } else {
                l
            }
        })
        .flat_map(|l| [l, "\n"])
        .collect();
    format!("{version}#define {stage}\n#define PARAMETER_UNIFORM\n{body}")
}

// RetroArch metadata is not GLSL: some ES drivers reject the quoted description even in a pragma.
// Use its default for uniforms, including those declared outside PARAMETER_UNIFORM guards.
fn parameter(line: &str) -> Option<(&str, f32)> {
    let rest = line.trim_start().strip_prefix("#pragma")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start().strip_prefix("parameter")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start();
    let end = rest.find(char::is_whitespace)?;
    let name = &rest[..end];
    let description = rest[end..].trim_start().strip_prefix('"')?;
    let end = description.find('"')?;
    let default = description[end + 1..]
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some((name, default))
}

/// The column-major matrix that takes the unit square onto `rect` in clip space.
pub fn mvp((x, y, w, h): (f32, f32, f32, f32)) -> [f32; 16] {
    let (sw, sh) = (OUT_W as f32, OUT_H as f32);
    [
        2.0 * w / sw,
        0.0,
        0.0,
        0.0,
        0.0,
        -2.0 * h / sh,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        2.0 * x / sw - 1.0,
        1.0 - 2.0 * y / sh,
        0.0,
        1.0,
    ]
}

/// Where on the texture a corner of the screen rect reads, for a picture turned `rot` quarter
/// turns counter-clockwise, as the built-in fragment shader's `turned` does.
fn turned((x, y): (f32, f32), rot: u32) -> (f32, f32) {
    match rot % 4 {
        3 => (y, 1.0 - x),
        2 => (1.0 - x, 1.0 - y),
        1 => (1.0 - y, x),
        _ => (x, y),
    }
}

/// The four strip corners, each its unit-square position then its texture coordinate.
pub fn corners(src: [f32; 4], rotation: u32) -> [f32; 16] {
    let mut out = [0.0; 16];
    for (i, corner) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
        .into_iter()
        .enumerate()
    {
        let (u, v) = turned(corner, rotation);
        out[i * 4..i * 4 + 4].copy_from_slice(&[
            corner.0,
            corner.1,
            src[0] + u * src[2],
            src[1] + v * src[3],
        ]);
    }
    out
}

impl CustomPass {
    pub fn new(src: &str, linear: bool) -> Result<Self, GfxError> {
        let es = crate::gl::es();
        let prog = crate::gl::program_with(
            &stage(src, "VERTEX", es),
            &stage(src, "FRAGMENT", es),
            &[(VERTEX_AT, "VertexCoord"), (TEX_AT, "TexCoord")],
        )?;
        let at = |name| crate::gl::uniform_location(prog, name);
        let pass = unsafe {
            gl::UseProgram(prog);
            gl::Uniform1i(at("Texture"), 0);
            for (name, value) in src.lines().filter_map(parameter) {
                gl::Uniform1f(at(name), value);
            }
            let mut vao = 0;
            if !es {
                gl::GenVertexArrays(1, &mut vao);
            }
            let mut vbo = 0;
            gl::GenBuffers(1, &mut vbo);
            CustomPass {
                prog,
                vao,
                vbo,
                linear,
                frames: Cell::new(0),
                u_mvp: at("MVPMatrix"),
                u_texture_size: at("TextureSize"),
                u_input_size: at("InputSize"),
                u_output_size: at("OutputSize"),
                u_frame_count: at("FrameCount"),
                u_frame_direction: at("FrameDirection"),
            }
        };
        Ok(pass)
    }

    pub fn draw(&self, tex: gl::types::GLuint, placed: &Placed) {
        let (_, _, w, h) = placed.rect;
        let (tw, th) = (placed.texels.0 as f32, placed.texels.1 as f32);
        let verts = corners(placed.src, placed.rotation);
        let frame = self.frames.get();
        self.frames.set(frame.wrapping_add(1));
        let filter = match self.linear {
            true => gl::LINEAR,
            false => gl::NEAREST,
        } as i32;
        unsafe {
            gl::UseProgram(self.prog);
            gl::UniformMatrix4fv(self.u_mvp, 1, gl::FALSE, mvp(placed.rect).as_ptr());
            gl::Uniform2f(self.u_texture_size, tw, th);
            gl::Uniform2f(self.u_input_size, tw, th);
            gl::Uniform2f(
                self.u_output_size,
                w * placed.fbo_scale[0],
                h * placed.fbo_scale[1],
            );
            gl::Uniform1i(self.u_frame_count, frame as i32);
            gl::Uniform1i(self.u_frame_direction, 1);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, tex);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, filter);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, filter);
            if self.vao != 0 {
                gl::BindVertexArray(self.vao);
            }
            gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                std::mem::size_of_val(&verts) as isize,
                verts.as_ptr() as *const std::ffi::c_void,
                gl::DYNAMIC_DRAW,
            );
            let stride = 4 * std::mem::size_of::<f32>() as i32;
            gl::EnableVertexAttribArray(VERTEX_AT);
            gl::VertexAttribPointer(VERTEX_AT, 2, gl::FLOAT, gl::FALSE, stride, std::ptr::null());
            gl::EnableVertexAttribArray(TEX_AT);
            gl::VertexAttribPointer(
                TEX_AT,
                2,
                gl::FLOAT,
                gl::FALSE,
                stride,
                (2 * std::mem::size_of::<f32>()) as *const std::ffi::c_void,
            );
            gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
            if self.vao != 0 {
                gl::BindVertexArray(0);
            } else {
                gl::DisableVertexAttribArray(TEX_AT);
            }
            // The game texture is sampled sharp everywhere else.
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
        }
    }
}

impl Drop for CustomPass {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteBuffers(1, &self.vbo);
            if self.vao != 0 {
                gl::DeleteVertexArrays(1, &self.vao);
            }
            gl::DeleteProgram(self.prog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_files_version_gives_way_to_the_gls_and_the_stage_is_defined() {
        let s = stage("#version 130\nvoid main() {}\n", "FRAGMENT", true);
        assert_eq!(
            s,
            "#version 100\n#define FRAGMENT\n#define PARAMETER_UNIFORM\n\nvoid main() {}\n"
        );
    }

    #[test]
    fn parameter_metadata_is_removed_and_defaults_are_read() {
        let line = "  #pragma parameter size \"Grid size\" 1.0 1.0 2.0 1.0";
        assert_eq!(parameter(line), Some(("size", 1.0)));
        assert!(!stage(line, "VERTEX", true).contains("Grid size"));
        assert_eq!(parameter("#pragma optimize(on)"), None);
    }

    #[test]
    fn the_matrix_takes_the_unit_square_onto_the_rect() {
        let m = mvp((0.0, 0.0, OUT_W as f32, OUT_H as f32));
        let at = |x: f32, y: f32| (m[0] * x + m[4] * y + m[12], m[1] * x + m[5] * y + m[13]);
        assert_eq!(at(0.0, 0.0), (-1.0, 1.0));
        assert_eq!(at(1.0, 1.0), (1.0, -1.0));
    }

    #[test]
    fn a_turned_picture_reads_its_corners_turned() {
        let c = corners([0.0, 0.0, 1.0, 1.0], 2);
        assert_eq!(&c[0..4], &[0.0, 0.0, 1.0, 1.0]);
        assert_eq!(&c[12..16], &[1.0, 1.0, 0.0, 0.0]);
    }
}
