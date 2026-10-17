"""Model and render the neutral shells of the media slot draws on its shelves.

Run inside Blender (5.0), headless:

    blender -b -P tools/media/render_media.py -- [name ...]

Every medium is modelled at its real size in millimetres, lit from the upper left and rendered
straight on through an orthographic camera in 50% grey plastic with a moulded stipple. The render
is then scaled so the plain face sits at exactly 128: slot blends a game's colour onto these with
an Overlay, so 128 gives the colour as it is, the highlights lift it and the shadows sink it, the
way the hand-drawn Game Boy shells already work. Each one lands in assets/<name>_neutral.png,
and the script prints the label box it measured, in pixels, for crates/slot-ui to paste the label
into.
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
    stipple: float = 1.0
    # Round recesses as (cx, cy, r, depth); a depth past the medium's own goes right through.
    circles: list[tuple[float, float, float, float]] = field(default_factory=list)
    # Round bosses standing on the face, as (cx, cy, r, height): buttons, spools, capacitors.
    knobs: list[tuple[float, float, float, float]] = field(default_factory=list)
    # Patches of the face in another shade, as a factor on the plastic's grey: a metal shutter
    # lighter, a chip or the tape behind a window darker. Rectangles, then circles.
    shades: list[tuple[Box, float]] = field(default_factory=list)
    round_shades: list[tuple[float, float, float, float]] = field(default_factory=list)
    # Patches the tool leaves polished, past the label: a shutter, a screen.
    smooth: list[Box] = field(default_factory=list)

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
    # The cuts leave the face one n-gon held together by long bridging edges, and the bevel slides
    # a rim's corner along whichever of those meets it, creasing the face in a diagonal line.
    # Even triangles first give it short edges to slide along instead.
    tri = body.modifiers.new("triangulate", "TRIANGULATE")
    tri.quad_method = "BEAUTY"
    tri.ngon_method = "BEAUTY"
    mod = body.modifiers.new("bevel", "BEVEL")
    mod.width = bevel
    mod.segments = 4
    mod.limit_method = "ANGLE"
    mod.angle_limit = math.radians(40)
    mod.harden_normals = True


class Graph:
    """A material's node tree, wired a node at a time."""

    def __init__(self, mat: bpy.types.Material) -> None:
        self.tree = mat.node_tree
        self.object = self.node("ShaderNodeTexCoord").outputs["Object"]
        xyz = self.node("ShaderNodeSeparateXYZ")
        self.link(self.object, xyz.inputs[0])
        self.x, self.y, self.z = xyz.outputs

    def node(self, kind: str, **inputs) -> bpy.types.Node:
        n = self.tree.nodes.new(kind)
        for key, value in inputs.items():
            if hasattr(n, key):
                setattr(n, key, value)
            else:
                n.inputs[key].default_value = value
        return n

    def link(self, out, into) -> None:
        self.tree.links.new(out, into)

    def math(self, op: str, a, b=None):
        n = self.node("ShaderNodeMath", operation=op)
        for sock, v in zip(n.inputs, (a, b)):
            if v is None:
                continue
            if isinstance(v, (int, float)):
                sock.default_value = v
            else:
                self.link(v, sock)
        return n.outputs[0]

    def noise(self, scale: float, detail: float = 2.0, rough: float = 0.6, coord=None):
        n = self.node("ShaderNodeTexNoise", **{"Scale": scale, "Detail": detail, "Roughness": rough})
        if "noise_dimensions" in dir(n):
            n.noise_dimensions = "3D"
        self.link(coord or self.object, n.inputs["Vector"])
        return n.outputs["Fac"]

    def inside(self, boxes: list[tuple[float, float, float, float]]):
        """1 inside any of the (x0, y0, x1, y1) world rectangles, else 0."""
        total = None
        for x0, y0, x1, y1 in boxes:
            a = self.math("MULTIPLY", self.math("GREATER_THAN", self.x, x0), self.math("LESS_THAN", self.x, x1))
            b = self.math("MULTIPLY", self.math("GREATER_THAN", self.y, y0), self.math("LESS_THAN", self.y, y1))
            both = self.math("MULTIPLY", a, b)
            total = both if total is None else self.math("MAXIMUM", total, both)
        return total


# The stipple of a moulded shell: the spark-eroded (EDM) finish of the tool leaves the plastic
# matte and finely pitted, which reads on screen as a salt-and-pepper grain a pixel or two across,
# the way the hand-drawn Game Boy shells have it.
# A grain is about this many pixels across at the size the shell is rendered, whatever the
# medium's own size: it is the pixel-sized speckle that reads as moulded plastic on screen.
GRAIN_PX = 1.15
STIPPLE_BUMP = 0.8
# How far the plastic's own shade wanders, either side of its grey.
MOTTLE = 0.28


def plastic(
    smooth: list[tuple[float, float, float, float]],
    px_per_mm: float,
    stipple: float = 1.0,
    grey: float = 0.214,
    shades: list[tuple[tuple[float, float, float, float], float]] = (),
    round_shades: list[tuple[float, float, float, float]] = (),
) -> bpy.types.Material:
    """Matte moulded plastic, stippled everywhere but the `smooth` rectangles (the label recess,
    which the label covers and the tool leaves polished). `stipple` scales the grain: a printed
    card is much smoother than a moulded shell."""
    mat = bpy.data.materials.new("plastic")
    mat.use_nodes = True
    g = Graph(mat)
    bsdf = g.tree.nodes["Principled BSDF"]
    rough_finish = g.math("SUBTRACT", 1.0, g.inside(smooth)) if smooth else None

    def textured(v):
        return g.math("MULTIPLY", v, rough_finish) if rough_finish is not None else v

    # The grains: cells about a pixel across, each its own depth and shade. Voronoi cells keep
    # their contrast at the size of a pixel, where a smooth noise would average away to grey.
    cells = g.node("ShaderNodeTexVoronoi", **{"Scale": px_per_mm / GRAIN_PX, "Randomness": 1.0})
    g.link(g.object, cells.inputs["Vector"])
    speck = g.math("ADD", cells.outputs["Color"], 0.0)
    pit = cells.outputs["Distance"]
    bump = g.node("ShaderNodeBump", **{"Distance": 0.07 * GRAIN_PX / px_per_mm})
    g.link(textured(g.math("ADD", STIPPLE_BUMP * stipple, 0.0)), bump.inputs["Strength"])
    g.link(g.math("ADD", g.math("MULTIPLY", pit, 0.6), g.math("MULTIPLY", speck, 0.4)), bump.inputs["Height"])
    g.link(bump.outputs["Normal"], bsdf.inputs["Normal"])

    # Mottle: the plastic's shade and sheen vary a little from grain to grain.
    wander = g.math("MULTIPLY", g.math("SUBTRACT", speck, 0.5), 2 * MOTTLE * stipple)
    # Mostly pits: a grain darkens more often than it catches the light, which also keeps a dark
    # shell from looking dusty once its colour is laid over.
    wander = textured(g.math("SUBTRACT", wander, g.math("MULTIPLY", g.math("MAXIMUM", wander, 0.0), 0.45)))
    base = g.math("MULTIPLY", g.math("ADD", wander, 1.0), grey)
    for box, factor in shades:
        base = mixf(g, base, g.math("MULTIPLY", base, factor), g.inside([box]))
    for cx, cy, r, factor in round_shades:
        dx, dy = g.math("SUBTRACT", g.x, cx), g.math("SUBTRACT", g.y, cy)
        within = g.math("LESS_THAN", g.math("ADD", g.math("MULTIPLY", dx, dx), g.math("MULTIPLY", dy, dy)), r * r)
        base = mixf(g, base, g.math("MULTIPLY", base, factor), within)
    g.link(base, bsdf.inputs["Base Color"])
    g.link(g.math("ADD", g.math("MULTIPLY", speck, 0.25), 0.45), bsdf.inputs["Roughness"])
    bsdf.inputs["Specular IOR Level"].default_value = 0.45
    return mat


def mixf(g: Graph, a, b, t):
    """a where t is 0, b where it is 1."""
    return g.math("ADD", a, g.math("MULTIPLY", g.math("SUBTRACT", b, a), t))


# The zones of a disc's top side, as radii in millimetres (a CD's, 60 mm across the half).
HOLE_R = 7.5
STACK_R = (16.0, 17.0)
PRINT_R = (22.5, 58.5)
MATRIX_R = 19.6


def disc_material(m: Medium) -> bpy.types.Material:
    """A pressed disc seen from its printed side: the clear clamping hub with its moulded rings,
    the mirror band where the aluminium shows through the polycarbonate, the print, and the clear
    rim past it."""
    mat = bpy.data.materials.new("disc")
    mat.use_nodes = True
    g = Graph(mat)
    bsdf = g.tree.nodes["Principled BSDF"]
    r = g.math("SQRT", g.math("ADD", g.math("MULTIPLY", g.x, g.x), g.math("MULTIPLY", g.y, g.y)))
    theta = g.math("ARCTAN2", g.y, g.x)

    # Print: matte ink with a little screen grain.
    ink = g.math("ADD", 1.0, g.math("MULTIPLY", g.math("SUBTRACT", g.noise(4.0, 1.0), 0.5), 0.12))
    printed = g.math("MULTIPLY", ink, 0.214)
    # Mirror band: the metal under the clear plastic shows the light as two bright wedges across
    # the centre, with fine lathe rings through them.
    wedge = g.math("POWER", g.math("ABSOLUTE", g.math("COSINE", g.math("SUBTRACT", theta, 2.36))), 6.0)
    lathe = g.math("MULTIPLY", g.math("SINE", g.math("MULTIPLY", r, 31.0)), 0.025)
    mirror = g.math("ADD", g.math("ADD", 0.17, g.math("MULTIPLY", wedge, 0.30)), lathe)
    # Hub and rim: clear plastic over nothing, a light grey with the moulding rings.
    rings = g.math("MULTIPLY", g.math("SINE", g.math("MULTIPLY", r, 9.0)), 0.035)
    clear = g.math("ADD", 0.36, rings)

    in_mirror = g.math("MULTIPLY", g.math("GREATER_THAN", r, STACK_R[1]), g.math("LESS_THAN", r, PRINT_R[0]))
    in_print = g.math("MULTIPLY", g.math("GREATER_THAN", r, PRINT_R[0]), g.math("LESS_THAN", r, PRINT_R[1]))
    shade = mixf(g, clear, mirror, in_mirror)
    shade = mixf(g, shade, printed, in_print)
    g.link(shade, bsdf.inputs["Base Color"])
    g.link(mixf(g, 0.18, 0.5, in_print), bsdf.inputs["Roughness"])

    bump = g.node("ShaderNodeBump", **{"Distance": 0.02})
    g.link(g.math("MULTIPLY", g.math("SUBTRACT", 1.0, in_print), 0.5), bump.inputs["Strength"])
    g.link(g.math("SINE", g.math("MULTIPLY", r, 9.0)), bump.inputs["Height"])
    g.link(bump.outputs["Normal"], bsdf.inputs["Normal"])
    return mat


def matrix_ring(z: float) -> None:
    """The mastering codes etched round the mirror band, a millimetre tall."""
    text = "  IFPI L021    0042-1 A1    MADE IN JAPAN    IFPI 9417  "
    ink = bpy.data.materials.new("etch")
    ink.use_nodes = True
    etch = ink.node_tree.nodes["Principled BSDF"]
    etch.inputs["Base Color"].default_value = (0.08, 0.08, 0.08, 1)
    etch.inputs["Roughness"].default_value = 0.8
    step = 2 * math.pi / len(text)
    for i, ch in enumerate(text):
        if ch == " ":
            continue
        curve = bpy.data.curves.new(f"matrix {i}", "FONT")
        curve.body = ch
        curve.size = 1.05
        curve.align_x = "CENTER"
        curve.extrude = 0.01
        obj = bpy.data.objects.new(f"matrix {i}", curve)
        # Clockwise from the top, each letter standing on the circle and facing out.
        a = math.pi / 2 - i * step
        obj.location = (MATRIX_R * math.cos(a), MATRIX_R * math.sin(a), z + 0.01)
        obj.rotation_euler = (0, 0, a - math.pi / 2)
        curve.materials.append(ink)
        bpy.context.collection.objects.link(obj)


def build(m: Medium) -> bpy.types.Object:
    if m.disc:
        body = cylinder("disc", m.w / 2, 0, m.depth)
        # The clear hub sits a step below the print, with the stacking ring raised on it.
        subtract(body, cylinder("hub", PRINT_R[0], m.depth - 0.12, m.depth + 1))
        unite(body, cylinder("ring", STACK_R[1], m.depth - 0.12, m.depth + 0.25))
        subtract(body, cylinder("ring hollow", STACK_R[0], m.depth - 0.12, m.depth + 1))
        # Last, so nothing above fills it back in.
        subtract(body, cylinder("hole", HOLE_R, -1, m.depth + 1))
        matrix_ring(m.depth - 0.12)
    else:
        outline = m.outline or rounded(rect(0, 0, m.w, m.h), m.corner)
        body = prism(m.name, m, outline, 0, m.depth)
        for i, (box, deep) in enumerate(m.cuts):
            shape = rounded(rect(box.x, box.y, box.w, box.h), min(box.w / 2.2, box.h / 2.2, 2.0), 4)
            subtract(body, prism(f"cut {i}", m, shape, m.depth - deep, m.depth + 1))
        for i, (box, tall) in enumerate(m.raised):
            shape = rounded(rect(box.x, box.y, box.w, box.h), min(box.w / 2.2, box.h / 2.2, 2.0), 4)
            unite(body, prism(f"rib {i}", m, shape, m.depth - 0.5, m.depth + tall))
        for i, (cx, cy, r, deep) in enumerate(m.circles):
            subtract(body, cylinder(f"well {i}", r, m.depth - deep, m.depth + 1, world(m, cx, cy)))
        for i, (cx, cy, r, tall) in enumerate(m.knobs):
            unite(body, cylinder(f"knob {i}", r, m.depth - 0.5, m.depth + tall, world(m, cx, cy)))
    finish(body, m.bevel)
    if m.disc:
        body.data.materials.append(disc_material(m))
    else:
        def area(b: Box) -> tuple[float, float, float, float]:
            x0, y1 = world(m, b.x, b.y)
            x1, y0 = world(m, b.x + b.w, b.y + b.h)
            return (x0, y0, x1, y1)

        body.data.materials.append(
            plastic(
                [area(b) for b in [m.label, *m.smooth]],
                m.size[0] / m.w,
                m.stipple,
                shades=[(area(b), f) for b, f in m.shades],
                round_shades=[(*world(m, cx, cy), r, f) for cx, cy, r, f in m.round_shades],
            )
        )
    return body


def stage(m: Medium) -> None:
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    # Enough samples to settle without the denoiser, which would smooth the stipple away.
    scene.cycles.samples = int(os.environ.get("UNSLOT_MEDIA_SAMPLES", 768))
    scene.cycles.use_denoising = False
    # A box filter a pixel wide keeps each grain in its own pixel rather than blurring it.
    scene.cycles.filter_width = 1.0
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

    # A lamp on the same side at arm's length, so the face falls off a little from its upper left
    # corner to its lower right, as it would on a desk, and recesses fill softly.
    span = max(m.w, m.h)
    lamp = bpy.data.objects.new("lamp", bpy.data.lights.new("lamp", "AREA"))
    lamp.data.size = span * 0.8
    lamp.location = (-span * 0.9, span * 0.9, span * 1.6)
    distance = Vector(lamp.location).length
    # Blender's units are metres to its lights: about a quarter of the sun's light at the centre.
    lamp.data.energy = 0.8 * math.pi * distance**2
    lamp.rotation_euler = Vector(lamp.location).to_track_quat("Z", "Y").to_euler()
    bpy.context.collection.objects.link(lamp)

    world_ =bpy.data.worlds.new("world")
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
            # Printed card, not moulded plastic.
            stipple=0.3,
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


def recess(b: Box, pad: float = 1.5, deep: float = 0.5) -> tuple[Box, float]:
    """The well a label sits in, `pad` round it."""
    return (Box(b.x - pad, b.y - pad, b.w + 2 * pad, b.h + 2 * pad), deep)


# How much darker a part is than the plastic: a chip's black epoxy, the tape behind a window.
DARK = 0.35
# A metal shutter or a contact, lighter than the plastic round it.
METAL = 1.8


def pcb() -> Medium:
    """An arcade game board: fibreglass under solder mask, the marquee sticker on it, ROMs and
    logic in rows, and the JAMMA edge of gold fingers along its bottom."""
    w, h = 150.0, 112.0
    label = Box(10, 9, 70, 44)
    # The board, then the edge tab the cabinet's connector takes.
    outline = rounded([(0, 0), (w, 0), (w, 100), (128, 100), (128, h), (22, h), (22, 100), (0, 100)], 2.0)
    chips = [Box(88 + col * 16, 10 + row * 22, 12, 18) for row in range(3) for col in range(3)]
    chips += [Box(12 + col * 14, 62, 10, 26) for col in range(5)]
    fingers = [Box(25 + i * 4.2, 102, 2.6, 9) for i in range(24)]
    return Medium(
        "pcb",
        w=w,
        h=h,
        depth=1.6,
        px=360,
        outline=outline,
        label=label,
        probe=Box(81, 58, 5, 20),
        bevel=0.3,
        stipple=0.55,
        raised=[(b, 3.5) for b in chips] + [(b, 0.1) for b in fingers],
        knobs=[(92, 88, 3.2, 6.0), (102, 88, 3.2, 6.0), (112, 88, 2.4, 4.0), (138, 88, 4.0, 7.0)],
        circles=[(5, 5, 2.2, 5.0), (w - 5, 5, 2.2, 5.0), (5, 95, 2.2, 5.0), (w - 5, 95, 2.2, 5.0)],
        shades=[(b, DARK) for b in chips] + [(b, METAL) for b in fingers],
        round_shades=[(92, 88, 3.2, 0.7), (102, 88, 3.2, 0.7), (112, 88, 2.4, 0.7), (138, 88, 4.0, 0.7)],
    )


def cart(
    name: str,
    w: float,
    h: float,
    depth: float,
    px: int,
    label: Box,
    probe: Box,
    outline: list[tuple[float, float]] | None = None,
    corner: float = 3.0,
    grip: list[tuple[Box, float]] = (),
    bevel: float = 1.4,
    **more,
) -> Medium:
    """A moulded cartridge with its label in a shallow well and a grip of grooves."""
    return Medium(
        name,
        w=w,
        h=h,
        depth=depth,
        px=px,
        outline=outline,
        corner=corner,
        label=label,
        probe=probe,
        bevel=bevel,
        cuts=[recess(label, 1.2 if w < 80 else 1.5, 0.4 if w < 80 else 0.5)] + list(grip),
        **more,
    )


def arched(w: float, h: float, rise: float, corner: float, steps: int = 12) -> list[tuple[float, float]]:
    """A box whose top edge bows up by `rise` in the middle."""
    top = [(w * i / steps, rise * (1 - (2 * i / steps - 1) ** 2)) for i in range(steps + 1)]
    top = [(x, rise - y) for x, y in top]
    pts = top + [(w, h), (0, h)]
    return rounded(pts, corner, 4)


def new_media() -> list[Medium]:
    out = [pcb()]

    mvs = Box(12, 26, 136, 74)
    out.append(
        cart(
            "mvs",
            160,
            112,
            24,
            360,
            mvs,
            Box(4, 106, 30, 3),
            corner=3,
            grip=ridges(14, 5, 132, 15, 8, across=True, deep=0.8),
        )
    )

    fds = Box(7, 7, 62, 38)
    out.append(
        Medium(
            "fds",
            w=76,
            h=90,
            depth=3.0,
            px=272,
            outline=rounded([(0, 0), (76, 0), (76, 84), (70, 90), (6, 90), (0, 84)], 2.5),
            label=fds,
            probe=Box(4, 60, 8, 12),
            bevel=0.5,
            cuts=[recess(fds, 1.2, 0.3), (Box(28, 52, 20, 32), 1.2)],
            shades=[(Box(28, 52, 20, 32), DARK)],
        )
    )

    n64 = Box(16, 24, 84, 44)
    out.append(
        cart(
            "n64",
            116,
            76,
            18,
            360,
            n64,
            Box(4, 70, 20, 3),
            outline=arched(116, 76, 5, 8),
            grip=ridges(18, 8, 80, 12, 6, across=True, deep=0.6),
        )
    )

    vb = Box(8, 24, 56, 48)
    out.append(
        cart(
            "vb",
            72,
            80,
            10,
            300,
            vb,
            Box(8, 74, 10, 3),
            corner=4,
            grip=ridges(10, 5, 52, 13, 5, across=True, deep=0.5),
        )
    )

    mini = Box(3, 8, 21, 14)
    out.append(
        cart(
            "pokemini",
            27,
            25,
            4.5,
            300,
            mini,
            Box(9, 23.5, 8, 0.8),
            corner=2.5,
            bevel=0.4,
            grip=ridges(5, 2.2, 17, 4, 3, across=True, deep=0.25),
        )
    )

    screen = Box(30, 10, 50, 40)
    out.append(
        Medium(
            "gw",
            w=110,
            h=67,
            depth=11,
            px=360,
            outline=None,
            corner=5,
            label=screen,
            probe=Box(40, 56, 30, 4),
            bevel=1.2,
            cuts=[recess(screen, 2.5, 0.8)],
            knobs=[(14, 44, 6.5, 1.6), (96, 44, 6.5, 1.6), (96, 10, 2.4, 0.8), (96, 18, 2.4, 0.8), (104, 10, 2.4, 0.8)],
            round_shades=[(14, 44, 6.5, 0.55), (96, 44, 6.5, 0.55)],
        )
    )

    x32 = Box(12, 54, 76, 72)
    out.append(
        cart(
            "32x",
            100,
            138,
            20,
            250,
            x32,
            Box(4, 134, 20, 2),
            outline=arched(100, 138, 12, 5),
            grip=ridges(14, 18, 72, 28, 8, across=True, deep=0.8),
        )
    )

    sg = Box(10, 30, 65, 60)
    out.append(
        cart(
            "sg1000",
            85,
            104,
            14,
            280,
            sg,
            Box(4, 99, 20, 2),
            corner=3,
            grip=ridges(12, 5, 61, 18, 6, across=True, deep=0.6),
        )
    )

    a26 = Box(8, 8, 64, 50)
    out.append(
        cart(
            "a2600",
            80,
            96,
            22,
            300,
            a26,
            Box(4, 60.5, 16, 2.5),
            outline=rounded([(0, 0), (80, 0), (80, 64), (74, 66), (74, 96), (6, 96), (6, 66), (0, 64)], 3),
            grip=ridges(10, 68, 60, 24, 7, across=True, deep=0.7),
        )
    )

    a52 = Box(10, 8, 85, 56)
    out.append(
        cart(
            "a5200",
            105,
            116,
            22,
            300,
            a52,
            Box(4, 111, 20, 2),
            corner=4,
            grip=ridges(12, 72, 81, 38, 9, across=True, deep=0.8),
        )
    )

    a78 = Box(8, 6, 64, 56)
    out.append(
        cart(
            "a7800",
            80,
            100,
            20,
            288,
            a78,
            Box(10, 95, 14, 2),
            outline=rounded([(0, 0), (80, 0), (80, 100), (0, 100)], 6),
            grip=ridges(10, 70, 60, 24, 5, across=True, deep=0.7),
        )
    )

    col = Box(8, 8, 66, 58)
    out.append(
        cart(
            "coleco",
            82,
            100,
            18,
            295,
            col,
            Box(20, 95, 20, 2),
            corner=3,
            grip=ridges(3, 72, 8, 24, 3, across=False, deep=0.6) + ridges(71, 72, 8, 24, 3, across=False, deep=0.6),
        )
    )

    intv = Box(6, 8, 50, 52)
    out.append(
        cart(
            "intv",
            62,
            126,
            14,
            177,
            intv,
            Box(4, 121, 12, 2),
            corner=4,
            grip=ridges(8, 68, 46, 50, 9, across=True, deep=0.6),
        )
    )

    vec = Box(8, 10, 64, 48)
    out.append(
        cart(
            "vectrex",
            80,
            94,
            16,
            300,
            vec,
            Box(14, 89, 14, 2),
            outline=rounded([(0, 0), (80, 0), (80, 94), (0, 94)], 10),
            grip=ridges(12, 64, 56, 24, 6, across=True, deep=0.6),
        )
    )

    ody = Box(10, 28, 75, 44)
    out.append(
        cart(
            "odyssey",
            95,
            80,
            14,
            340,
            ody,
            Box(4, 75, 14, 2),
            corner=3,
            # The finger hole the cartridge is pulled out by.
            grip=[(Box(36, 6, 23, 12), 20.0)],
        )
    )

    chf = Box(10, 10, 80, 54)
    out.append(
        cart(
            "channelf",
            100,
            112,
            22,
            300,
            chf,
            Box(4, 106, 14, 2),
            outline=rounded([(0, 0), (100, 0), (100, 112), (60, 112), (56, 106), (44, 106), (40, 112), (0, 112)], 3),
            grip=ridges(12, 72, 76, 26, 7, across=True, deep=0.8),
        )
    )

    sv = Box(6, 6, 48, 30)
    out.append(
        cart(
            "supervision",
            60,
            56,
            9,
            320,
            sv,
            Box(14, 52.5, 20, 1.5),
            corner=3,
            bevel=0.9,
            grip=ridges(8, 41, 44, 11, 4, across=True, deep=0.5),
        )
    )

    duck = Box(6, 14, 46, 36)
    out.append(
        cart(
            "megaduck",
            58,
            64,
            8,
            300,
            duck,
            Box(3, 60, 8, 2),
            outline=rounded([(0, 0), (52, 0), (58, 6), (58, 64), (0, 64)], 3),
            bevel=0.9,
            grip=ridges(8, 3, 40, 8, 3, across=True, deep=0.4),
        )
    )

    msx = Box(30, 8, 82, 46)
    out.append(
        cart(
            "msx",
            120,
            64,
            14,
            360,
            msx,
            Box(30, 59, 20, 2),
            outline=rounded([(6, 0), (114, 0), (120, 6), (120, 64), (0, 64), (0, 6)], 2.5),
            grip=ridges(6, 8, 20, 48, 5, across=False, deep=0.6),
        )
    )

    # 3.5" disk: the shutter along the top, the label well under it, the corner cut.
    f35 = Box(10, 46, 70, 44)
    shutter = Box(24, 0, 42, 38)
    out.append(
        Medium(
            "floppy35",
            w=90,
            h=94,
            depth=3.3,
            px=300,
            outline=rounded([(0, 0), (86, 0), (90, 4), (90, 94), (0, 94)], 1.5),
            label=f35,
            probe=Box(3, 30, 6, 10),
            bevel=0.5,
            # The shutter's window, where the disk shows when it slides open, sits through it.
            cuts=[recess(f35, 1.0, 0.25), (Box(4, 84, 4, 6), 5.0), (Box(82, 84, 4, 6), 5.0), (Box(39, 5, 12, 25), 0.55)],
            raised=[(shutter, 0.15)],
            shades=[(shutter, 1.45), (Box(39, 5, 12, 25), DARK)],
            smooth=[shutter],
            stipple=0.8,
        )
    )

    # 5.25" disk: a paper sleeve with the hub, index hole and read slot showing the disk inside.
    f525 = Box(10, 6, 113, 22)
    out.append(
        Medium(
            "floppy525",
            w=133,
            h=133,
            depth=1.4,
            px=330,
            outline=None,
            corner=1.5,
            label=f525,
            probe=Box(8, 40, 14, 14),
            bevel=0.25,
            stipple=0.4,
            # The write-protect notch is cut in from the right-hand edge.
            cuts=[(Box(59, 92, 15, 36), 0.9), (Box(127, 34, 10, 8), 5.0)],
            circles=[(66.5, 66.5, 16.5, 0.8), (66.5, 66.5, 14.0, 1.0), (88, 64, 3.0, 0.9)],
            shades=[(Box(59, 92, 15, 36), DARK)],
            round_shades=[(66.5, 66.5, 14.0, 0.5), (88, 64, 3.0, DARK)],
        )
    )

    # Compact cassette: the label across the top with the window through it, the spools, the
    # head opening's trapezoid along the bottom.
    tape_label = Box(8, 6, 84, 22)
    out.append(
        Medium(
            "tape",
            w=100,
            h=64,
            depth=12,
            px=360,
            outline=None,
            corner=3,
            label=tape_label,
            probe=Box(2.5, 30, 3, 20),
            bevel=1.0,
            cuts=[
                recess(tape_label, 1.2, 0.3),
                (Box(30, 32, 40, 14), 1.5),
                (Box(26, 52, 48, 12), 0.6),
            ],
            circles=[(22, 39, 7.5, 1.5), (78, 39, 7.5, 1.5), (4, 4, 1.3, 0.8), (96, 4, 1.3, 0.8), (50, 58, 1.3, 0.8), (4, 60, 1.3, 0.8), (96, 60, 1.3, 0.8)],
            knobs=[(22, 39, 3.6, 0.0), (78, 39, 3.6, 0.0)],
            shades=[(Box(30, 32, 40, 14), DARK)],
            round_shades=[(22, 39, 7.5, 0.6), (78, 39, 7.5, 0.6)],
        )
    )

    # Amstrad's 3" disk: a stiff case, label on top, the sliding shutter low down.
    cpc = Box(8, 8, 64, 50)
    cpc_shutter = Box(14, 66, 52, 30)
    out.append(
        Medium(
            "cpc3",
            w=80,
            h=100,
            depth=5,
            px=288,
            outline=None,
            corner=3,
            label=cpc,
            probe=Box(3, 70, 6, 10),
            bevel=0.7,
            cuts=[recess(cpc, 1.2, 0.3), (Box(34, 70, 12, 22), 0.6)],
            raised=[(cpc_shutter, 0.2)],
            shades=[(cpc_shutter, 1.35), (Box(34, 70, 12, 22), DARK)],
            smooth=[cpc_shutter],
        )
    )

    # UMD: the round-topped shell with its label on the front.
    umd = Box(8, 14, 48, 40)
    out.append(
        cart(
            "umd",
            64,
            65,
            4.2,
            320,
            umd,
            Box(3, 61, 10, 2),
            outline=arched(64, 65, 8, 6),
            bevel=0.6,
        )
    )

    fan = Box(9, 10, 52, 42)
    out.append(
        cart(
            "fantasy",
            70,
            76,
            12,
            320,
            fan,
            Box(4, 30, 3, 20),
            corner=8,
            bevel=2.0,
            grip=ridges(12, 60, 46, 12, 4, across=True, deep=0.6),
        )
    )
    return out


def main() -> None:
    wanted = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    for m in media() + new_media():
        if wanted and m.name not in wanted:
            continue
        render(m)


main()
