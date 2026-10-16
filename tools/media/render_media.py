"""Model and render the neutral shells of the media slot draws on its shelves.

Run inside Blender (5.0), headless:

    blender -b -P tools/media/render_media.py -- [name ...]

Every medium is modelled at its real size in millimetres, lit from the upper left and rendered
straight on through an orthographic camera in flat 50% grey plastic. The render is then scaled so
the plain face sits at exactly 128: slot blends a game's colour onto these with an Overlay, so 128
gives the colour as it is, the highlights lift it and the shadows sink it, the way the hand-drawn
Game Boy shells already work. Each one lands in assets/<name>_neutral.png, and the script prints
the label box it measured, in pixels, for crates/slot-ui to paste the label into.
"""

from __future__ import annotations

import math
import os
import sys
from dataclasses import dataclass, field
from pathlib import Path

import bmesh
import bpy
import numpy as np
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "assets"
SCRATCH = Path(os.environ.get("UNSLOT_MEDIA_SCRATCH", "/tmp/unslot-media"))

# The flat face of every shell is brought to this grey once it is rendered.
NEUTRAL = 128


@dataclass
class Box:
    """A rectangle in the medium's own millimetres: x right and y down from its top left."""

    x: float
    y: float
    w: float
    h: float


@dataclass
class Medium:
    name: str
    w: float
    h: float
    depth: float
    px: int
    outline: list[tuple[float, float]] | None
    label: Box
    probe: Box
    corner: float = 3.0
    bevel: float = 1.4
    cuts: list[tuple[Box, float]] = field(default_factory=list)
    raised: list[tuple[Box, float]] = field(default_factory=list)
    disc: bool = False

    @property
    def size(self) -> tuple[int, int]:
        return self.px, round(self.px * self.h / self.w)


def world(m: Medium, x: float, y: float) -> tuple[float, float]:
    return x - m.w / 2, m.h / 2 - y


def rounded(points: list[tuple[float, float]], radius: float, steps: int = 8) -> list[tuple[float, float]]:
    """The polygon through `points` with every corner filleted to `radius`."""
    out: list[tuple[float, float]] = []
    n = len(points)
    for i, here in enumerate(points):
        prev, nxt = Vector(points[i - 1]), Vector(points[(i + 1) % n])
        here = Vector(here)
        a, b = (prev - here), (nxt - here)
        r = min(radius, a.length / 2, b.length / 2)
        a.normalize()
        b.normalize()
        start, end = here + a * r, here + b * r
        for s in range(steps + 1):
            t = s / steps
            # A quadratic Bezier with the corner as its control point: close to an arc and never
            # past the corner, whatever the angle.
            p = start * (1 - t) ** 2 + here * 2 * t * (1 - t) + end * t**2
            out.append((p.x, p.y))
    return out


def rect(x: float, y: float, w: float, h: float) -> list[tuple[float, float]]:
    return [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]


def prism(name: str, m: Medium, outline: list[tuple[float, float]], z0: float, z1: float) -> bpy.types.Object:
    mesh = bpy.data.meshes.new(name)
    bm = bmesh.new()
    verts = [bm.verts.new((*world(m, x, y), z0)) for x, y in outline]
    face = bm.faces.new(verts)
    bmesh.ops.recalc_face_normals(bm, faces=[face])
    if face.normal.z > 0:
        face.normal_flip()
    grown = bmesh.ops.extrude_face_region(bm, geom=[face])
    top = [v for v in grown["geom"] if isinstance(v, bmesh.types.BMVert)]
    bmesh.ops.translate(bm, verts=top, vec=(0, 0, z1 - z0))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.to_mesh(mesh)
    bm.free()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return obj


def cylinder(name: str, r: float, z0: float, z1: float, at: tuple[float, float] = (0, 0)) -> bpy.types.Object:
    mesh = bpy.data.meshes.new(name)
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=256, radius1=r, radius2=r, depth=z1 - z0)
    bmesh.ops.translate(bm, verts=bm.verts[:], vec=(at[0], at[1], (z0 + z1) / 2))
    bm.to_mesh(mesh)
    bm.free()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    return obj


def cutter(obj: bpy.types.Object) -> bpy.types.Object:
    obj.hide_render = True
    obj.display_type = "WIRE"
    return obj


def subtract(body: bpy.types.Object, cut: bpy.types.Object) -> None:
    mod = body.modifiers.new(f"cut {cut.name}", "BOOLEAN")
    mod.operation = "DIFFERENCE"
    mod.solver = "EXACT"
    mod.object = cutter(cut)


def unite(body: bpy.types.Object, part: bpy.types.Object) -> None:
    mod = body.modifiers.new(f"join {part.name}", "BOOLEAN")
    mod.operation = "UNION"
    mod.solver = "EXACT"
    mod.object = cutter(part)


def finish(body: bpy.types.Object, bevel: float) -> None:
    mod = body.modifiers.new("bevel", "BEVEL")
    mod.width = bevel
    mod.segments = 4
    mod.limit_method = "ANGLE"
    mod.angle_limit = math.radians(40)
    mod.harden_normals = True


def plastic() -> bpy.types.Material:
    mat = bpy.data.materials.new("plastic")
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (0.214, 0.214, 0.214, 1)
    bsdf.inputs["Roughness"].default_value = 0.55
    return mat


def build(m: Medium) -> bpy.types.Object:
    if m.disc:
        body = cylinder("disc", m.w / 2, 0, m.depth)
        # The clear hub sits a step below the print, with the stacking ring raised on it.
        subtract(body, cylinder("hub", 22.5, m.depth - 0.12, m.depth + 1))
        unite(body, cylinder("ring", 17.0, m.depth - 0.12, m.depth + 0.25))
        subtract(body, cylinder("ring hollow", 16.0, m.depth - 0.12, m.depth + 1))
        # Last, so nothing above fills it back in.
        subtract(body, cylinder("hole", 7.5, -1, m.depth + 1))
    else:
        outline = m.outline or rounded(rect(0, 0, m.w, m.h), m.corner)
        body = prism(m.name, m, outline, 0, m.depth)
        for i, (box, deep) in enumerate(m.cuts):
            shape = rounded(rect(box.x, box.y, box.w, box.h), min(box.w / 2.2, box.h / 2.2, 2.0), 4)
            subtract(body, prism(f"cut {i}", m, shape, m.depth - deep, m.depth + 1))
        for i, (box, tall) in enumerate(m.raised):
            shape = rounded(rect(box.x, box.y, box.w, box.h), min(box.w / 2.2, box.h / 2.2, 2.0), 4)
            unite(body, prism(f"rib {i}", m, shape, m.depth - 0.5, m.depth + tall))
    finish(body, m.bevel)
    body.data.materials.append(plastic())
    return body


def stage(m: Medium) -> None:
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = 96
    scene.cycles.use_denoising = True
    scene.render.film_transparent = True
    scene.view_settings.view_transform = "Standard"
    scene.view_settings.look = "None"
    w, h = m.size
    scene.render.resolution_x = w
    scene.render.resolution_y = h
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"

    cam = bpy.data.objects.new("camera", bpy.data.cameras.new("camera"))
    cam.data.type = "ORTHO"
    # With the sensor fit left on Auto, the scale spans the longer side of the frame.
    cam.data.ortho_scale = max(m.w, m.h)
    cam.location = (0, 0, 200)
    bpy.context.collection.objects.link(cam)
    scene.camera = cam

    # The key: high on the upper left, so top and left edges catch the light and the bottom and
    # right fall into shade, as on the hand-drawn shells.
    sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
    sun.data.energy = 3.2
    sun.data.angle = math.radians(18)
    sun.rotation_euler = Vector((-1.0, 1.0, 1.1)).to_track_quat("Z", "Y").to_euler()
    bpy.context.collection.objects.link(sun)

    world_ = bpy.data.worlds.new("world")
    world_.use_nodes = True
    world_.node_tree.nodes["Background"].inputs["Color"].default_value = (1, 1, 1, 1)
    world_.node_tree.nodes["Background"].inputs["Strength"].default_value = 0.12
    scene.world = world_


def calibrate(m: Medium, raw: Path, out: Path) -> None:
    img = bpy.data.images.load(str(raw))
    w, h = img.size
    px = np.empty(w * h * 4, dtype=np.float32)
    img.pixels.foreach_get(px)
    # Blender stores rows bottom up. Flip so row 0 is the top, as in the box coordinates.
    rgba = px.reshape(h, w, 4)[::-1].copy()
    grey = rgba[..., :3].mean(axis=2)
    scale = m.size[0] / m.w
    b = m.probe
    probe = grey[int(b.y * scale) : int((b.y + b.h) * scale), int(b.x * scale) : int((b.x + b.w) * scale)]
    alpha = rgba[int(b.y * scale) : int((b.y + b.h) * scale), int(b.x * scale) : int((b.x + b.w) * scale), 3]
    level = float(np.median(probe[alpha > 0.99]))
    gain = (NEUTRAL / 255) / level
    shaded = np.clip(grey * gain, 0, 1)
    rgba[..., 0] = shaded
    rgba[..., 1] = shaded
    rgba[..., 2] = shaded
    flat = rgba[::-1].reshape(-1)
    done = bpy.data.images.new(out.stem, w, h, alpha=True)
    done.pixels.foreach_set(flat)
    done.filepath_raw = str(out)
    done.file_format = "PNG"
    done.save()
    lx, ly = round(m.label.x * scale), round(m.label.y * scale)
    lw, lh = round(m.label.w * scale), round(m.label.h * scale)
    print(f"MEDIUM {m.name} {w}x{h} label {lx} {ly} {lw} {lh} gain {gain:.3f}")


def render(m: Medium) -> None:
    bpy.ops.wm.read_factory_settings(use_empty=True)
    build(m)
    stage(m)
    SCRATCH.mkdir(parents=True, exist_ok=True)
    raw = SCRATCH / f"{m.name}_raw.png"
    bpy.context.scene.render.filepath = str(raw)
    bpy.ops.render.render(write_still=True)
    calibrate(m, raw, ASSETS / f"{m.name}_neutral.png")


def ridges(x: float, y: float, w: float, h: float, n: int, across: bool, deep: float = 0.6) -> list[tuple[Box, float]]:
    """`n` grooves in the box, running across it (horizontal) or down it (vertical)."""
    out = []
    for i in range(n):
        if across:
            pitch = h / n
            out.append((Box(x, y + i * pitch + pitch * 0.3, w, pitch * 0.4), deep))
        else:
            pitch = w / n
            out.append((Box(x + i * pitch + pitch * 0.3, y, pitch * 0.4, h), deep))
    return out


def media() -> list[Medium]:
    nes_label = Box(10, 8, 100, 82)
    snes_label = Box(30, 20, 68, 52)
    gen_label = Box(10, 40, 80, 80)
    sms_label = Box(42, 6, 78, 52)
    gg_label = Box(12, 15, 44, 34)
    hucard_label = Box(4, 4, 46, 66)
    ngp_label = Box(7, 18, 35, 29)
    ws_label = Box(5, 6, 34, 36)
    lynx_label = Box(7, 16, 52, 26)
    return [
        Medium(
            "nes",
            w=120,
            h=133,
            depth=17,
            px=304,
            outline=None,
            corner=2.5,
            label=nes_label,
            probe=Box(15, 96, 30, 8),
            cuts=[(Box(nes_label.x - 1.5, nes_label.y - 1.5, nes_label.w + 3, nes_label.h + 3), 0.5)]
            + ridges(8, 108, 104, 18, 7, across=True),
        ),
        Medium(
            "snes",
            w=128,
            h=92,
            depth=20,
            px=360,
            outline=rounded(
                [(6, 0), (122, 0), (128, 10), (128, 92), (0, 92), (0, 10)],
                5,
            ),
            label=snes_label,
            probe=Box(8, 30, 14, 30),
            cuts=[(Box(snes_label.x - 1.5, snes_label.y - 1.5, snes_label.w + 3, snes_label.h + 3), 0.5)]
            + ridges(6, 22, 16, 48, 4, across=False)
            + ridges(106, 22, 16, 48, 4, across=False),
        ),
        Medium(
            "genesis",
            w=100,
            h=126,
            depth=19,
            px=267,
            outline=rounded(
                [(0, 0), (100, 0), (100, 126), (0, 126)],
                4,
            ),
            label=gen_label,
            probe=Box(4, 124 - 6, 20, 3),
            cuts=[(Box(gen_label.x - 1.5, gen_label.y - 1.5, gen_label.w + 3, gen_label.h + 3), 0.5)]
            + ridges(8, 5, 84, 28, 9, across=True, deep=0.8),
        ),
        Medium(
            "sms",
            w=128,
            h=64,
            depth=12,
            px=360,
            outline=None,
            corner=3,
            label=sms_label,
            probe=Box(30, 59, 10, 3),
            cuts=[(Box(sms_label.x - 1.5, sms_label.y - 1.5, sms_label.w + 3, sms_label.h + 3), 0.5)]
            + ridges(6, 8, 28, 48, 6, across=False),
        ),
        Medium(
            "gamegear",
            w=68,
            h=54,
            depth=10,
            px=320,
            outline=rounded([(0, 4), (68, 4), (68, 54), (0, 54)], 3),
            label=gg_label,
            probe=Box(4, 50, 10, 2),
            cuts=[(Box(gg_label.x - 1.2, gg_label.y - 1.2, gg_label.w + 2.4, gg_label.h + 2.4), 0.4)]
            + ridges(8, 5.5, 52, 7, 4, across=True, deep=0.5),
        ),
        Medium(
            "hucard",
            w=54,
            h=85,
            depth=1.6,
            px=212,
            outline=None,
            corner=3,
            bevel=0.35,
            label=hucard_label,
            probe=Box(6, 74, 12, 4),
            cuts=[(Box(4, 76, 46, 6), 0.25)],
        ),
        Medium(
            "ngp",
            w=50,
            h=52,
            depth=9,
            px=300,
            outline=rounded([(4, 0), (46, 0), (50, 8), (50, 52), (0, 52), (0, 8)], 3),
            label=ngp_label,
            probe=Box(3, 49, 8, 2),
            cuts=[(Box(ngp_label.x - 1.2, ngp_label.y - 1.2, ngp_label.w + 2.4, ngp_label.h + 2.4), 0.4)]
            + ridges(8, 3, 34, 11, 5, across=False, deep=0.5),
        ),
        Medium(
            "wonderswan",
            w=60,
            h=48,
            depth=8,
            px=340,
            outline=None,
            corner=2.5,
            label=ws_label,
            probe=Box(42, 44, 10, 2),
            cuts=[(Box(ws_label.x - 1.2, ws_label.y - 1.2, ws_label.w + 2.4, ws_label.h + 2.4), 0.4)]
            + ridges(44, 6, 12, 36, 6, across=True, deep=0.5),
        ),
        Medium(
            "lynx",
            w=66,
            h=46,
            depth=6,
            px=340,
            outline=rounded(
                [(0, 6), (10, 0), (56, 0), (66, 6), (66, 46), (0, 46)],
                4,
            ),
            label=lynx_label,
            probe=Box(3, 43, 8, 2),
            cuts=[(Box(lynx_label.x - 1.2, lynx_label.y - 1.2, lynx_label.w + 2.4, lynx_label.h + 2.4), 0.4)]
            + ridges(14, 3, 38, 9, 4, across=True, deep=0.5),
        ),
        Medium(
            "disc",
            w=120,
            h=120,
            depth=1.2,
            px=337,
            outline=None,
            bevel=0.3,
            disc=True,
            label=Box(0, 0, 120, 120),
            probe=Box(52, 3, 16, 4),
        ),
    ]


def main() -> None:
    wanted = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    for m in media():
        if wanted and m.name not in wanted:
            continue
        render(m)


main()
