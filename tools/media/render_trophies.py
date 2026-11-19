"""Model and render the skeuomorphic RetroAchievements Trophy Room and 3D trophies.

Run inside Blender (5.0.1), headless:

    /home/clxud/.local/bin/blender -b -P tools/media/render_trophies.py
    /home/clxud/.local/bin/blender -b -P tools/media/render_trophies.py -- room platinum

Outputs into assets/:
  - trophy_room.png     (720x720 RGBA, opaque walnut & velvet display cabinet)
  - trophy_platinum.png (112x144 RGBA, transparent studio render w/ contact shadow)
  - trophy_gold.png     (112x144 RGBA)
  - trophy_silver.png   (112x144 RGBA)
  - trophy_bronze.png   (112x144 RGBA)
  - trophy_locked.png   (112x144 RGBA)
  - trophy_plaque.png   (120x120 RGBA, upright walnut/brass stand with dark square bezel)

Coordinates use top-down pixel space (x right, y down) matching UI composition.
"""

from __future__ import annotations

import math
import os
import sys
from pathlib import Path

import bmesh
import bpy
from bpy_extras.object_utils import world_to_camera_view
from mathutils import Euler, Matrix, Vector
import numpy as np

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "assets"

SAMPLES_ROOM = int(os.environ.get("UNSLOT_TROPHY_ROOM_SAMPLES", 320))
SAMPLES_SPRITE = int(os.environ.get("UNSLOT_TROPHY_SPRITE_SAMPLES", 256))


# ---------------------------------------------------------------------------
# Node-graph helpers
# ---------------------------------------------------------------------------


class MatGraph:
    """Thin wrapper for wiring Blender 5.0 shader node trees."""

    def __init__(self, name: str) -> None:
        self.mat = bpy.data.materials.new(name)
        self.mat.use_nodes = True
        self.tree = self.mat.node_tree
        self.bsdf = self.tree.nodes["Principled BSDF"]
        tc = self.node("ShaderNodeTexCoord")
        self.obj = tc.outputs["Object"]
        self.gen = tc.outputs["Generated"]
        info = self.node("ShaderNodeObjectInfo")
        self.rand = info.outputs["Random"]

    def node(self, kind: str, **attrs) -> bpy.types.Node:
        n = self.tree.nodes.new(kind)
        for k, v in attrs.items():
            if hasattr(n, k):
                setattr(n, k, v)
            elif k in n.inputs:
                n.inputs[k].default_value = v
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

    def ramp(self, fac, stops: list[tuple[float, tuple[float, float, float, float]]]):
        cr = self.node("ShaderNodeValToRGB")
        self.link(fac, cr.inputs["Factor"])
        elts = cr.color_ramp.elements
        while len(elts) < len(stops):
            elts.new(0.5)
        for elt, (pos, col) in zip(elts, stops):
            elt.position = pos
            elt.color = col
        return cr.outputs["Color"]

    def mix_rgba(self, fac, a, b):
        mx = self.node("ShaderNodeMix")
        mx.data_type = "RGBA"
        in_fac = mx.inputs[0]
        rgba_ins = [s for s in mx.inputs if s.type == "RGBA"]
        rgba_out = [s for s in mx.outputs if s.type == "RGBA"][0]
        if isinstance(fac, (int, float)):
            in_fac.default_value = fac
        else:
            self.link(fac, in_fac)
        for sock, val in zip(rgba_ins, (a, b)):
            if isinstance(val, tuple):
                sock.default_value = val
            else:
                self.link(val, sock)
        return rgba_out

    def noise(self, vec, scale: float, detail: float = 6.0, rough: float = 0.65, dist: float = 0.0):
        n = self.node("ShaderNodeTexNoise")
        n.inputs["Scale"].default_value = scale
        n.inputs["Detail"].default_value = detail
        n.inputs["Roughness"].default_value = rough
        n.inputs["Distortion"].default_value = dist
        self.link(vec, n.inputs["Vector"])
        return n.outputs["Factor"]

    def mapped(self, scale_xyz: tuple[float, float, float], rand_shift: float = 37.0):
        mp = self.node("ShaderNodeMapping")
        mp.inputs["Scale"].default_value = scale_xyz
        comb = self.node("ShaderNodeCombineXYZ")
        self.link(self.math("MULTIPLY", self.rand, rand_shift), comb.inputs["X"])
        self.link(self.math("MULTIPLY", self.rand, rand_shift * 0.71), comb.inputs["Y"])
        self.link(self.math("MULTIPLY", self.rand, rand_shift * 1.13), comb.inputs["Z"])
        self.link(comb.outputs["Vector"], mp.inputs["Location"])
        self.link(self.obj, mp.inputs["Vector"])
        return mp.outputs["Vector"]

    def bump(self, height, distance: float, strength: float = 0.5) -> None:
        b = self.node("ShaderNodeBump")
        b.inputs["Distance"].default_value = distance
        b.inputs["Strength"].default_value = strength
        self.link(height, b.inputs["Height"])
        self.link(b.outputs["Normal"], self.bsdf.inputs["Normal"])


# ---------------------------------------------------------------------------
# Procedural Cycles PBR Materials
# ---------------------------------------------------------------------------


def mat_walnut(name: str = "walnut", axis: str = "X", dark_bias: float = 0.0) -> bpy.types.Material:
    """Rich satin-oiled dark American walnut with directional grain and pores."""
    g = MatGraph(name)
    if axis == "X":
        stretch = (0.014, 0.14, 0.14)
        band_dir = "Y"
    elif axis == "Z":
        stretch = (0.14, 0.14, 0.014)
        band_dir = "X"
    else:
        stretch = (0.14, 0.014, 0.14)
        band_dir = "X"

    vec = g.mapped(stretch)
    wave = g.node("ShaderNodeTexWave")
    wave.wave_type = "BANDS"
    wave.bands_direction = band_dir
    wave.wave_profile = "SIN"
    wave.inputs["Scale"].default_value = 0.55
    wave.inputs["Distortion"].default_value = 5.5
    wave.inputs["Detail"].default_value = 6.0
    wave.inputs["Detail Scale"].default_value = 1.6
    wave.inputs["Detail Roughness"].default_value = 0.65
    g.link(vec, wave.inputs["Vector"])

    fine = g.noise(g.mapped((0.04, 0.45, 0.45) if axis == "X" else (0.45, 0.45, 0.04)), scale=1.8, detail=8.0, rough=0.75)
    grain = g.math("ADD", g.math("MULTIPLY", wave.outputs["Fac"], 0.68), g.math("MULTIPLY", fine, 0.32))

    # Deeper tone bias for inset plaques and shadowed moldings
    k = 1.0 - 0.42 * dark_bias
    col = g.ramp(
        grain,
        [
            (0.08, (0.022 * k, 0.010 * k, 0.005 * k, 1.0)),
            (0.32, (0.052 * k, 0.023 * k, 0.011 * k, 1.0)),
            (0.62, (0.098 * k, 0.044 * k, 0.021 * k, 1.0)),
            (0.90, (0.148 * k, 0.068 * k, 0.032 * k, 1.0)),
        ],
    )
    g.link(col, g.bsdf.inputs["Base Color"])
    rough = g.math("ADD", 0.36, g.math("MULTIPLY", grain, 0.16))
    g.link(rough, g.bsdf.inputs["Roughness"])
    g.bsdf.inputs["Specular IOR Level"].default_value = 0.44
    g.bsdf.inputs["Coat Weight"].default_value = 0.14
    g.bsdf.inputs["Coat Roughness"].default_value = 0.28
    g.bump(grain, distance=0.22, strength=0.35)
    return g.mat


def mat_oak_dark() -> bpy.types.Material:
    """Warm smoked dark oak for the bronze trophy plinth."""
    g = MatGraph("oak_dark")
    vec = g.mapped((0.025, 0.20, 0.20))
    n = g.noise(vec, scale=1.4, detail=8.0, rough=0.72, dist=1.2)
    col = g.ramp(
        n,
        [
            (0.18, (0.024, 0.013, 0.007, 1.0)),
            (0.55, (0.058, 0.032, 0.016, 1.0)),
            (0.88, (0.095, 0.054, 0.026, 1.0)),
        ],
    )
    g.link(col, g.bsdf.inputs["Base Color"])
    g.link(g.math("ADD", 0.38, g.math("MULTIPLY", n, 0.16)), g.bsdf.inputs["Roughness"])
    g.bsdf.inputs["Coat Weight"].default_value = 0.15
    g.bsdf.inputs["Coat Roughness"].default_value = 0.30
    g.bump(n, distance=0.20, strength=0.35)
    return g.mat


def mat_velvet() -> bpy.types.Material:
    """Deep oxblood-burgundy museum velvet backing with fine nap sheen."""
    g = MatGraph("velvet")
    nap = g.noise(g.obj, scale=0.45, detail=7.0, rough=0.80)
    mottle = g.noise(g.obj, scale=0.025, detail=3.0, rough=0.55)
    fac = g.math("ADD", g.math("MULTIPLY", nap, 0.65), g.math("MULTIPLY", mottle, 0.35))
    col = g.ramp(
        fac,
        [
            (0.15, (0.048, 0.008, 0.012, 1.0)),
            (0.55, (0.088, 0.016, 0.022, 1.0)),
            (0.88, (0.125, 0.024, 0.030, 1.0)),
        ],
    )
    g.link(col, g.bsdf.inputs["Base Color"])
    g.bsdf.inputs["Roughness"].default_value = 0.88
    g.bsdf.inputs["Specular IOR Level"].default_value = 0.15
    g.bsdf.inputs["Sheen Weight"].default_value = 0.90
    g.bsdf.inputs["Sheen Roughness"].default_value = 0.42
    g.bsdf.inputs["Sheen Tint"].default_value = (0.72, 0.24, 0.26, 1.0)
    g.bump(nap, distance=0.14, strength=0.40)
    return g.mat


def mat_brushed_metal(
    name: str,
    color_a: tuple[float, float, float, float],
    color_b: tuple[float, float, float, float],
    rough_base: float,
    rough_span: float,
    brush_scale: tuple[float, float, float] = (0.08, 0.9, 0.9),
    bump_dist: float = 0.05,
    metallic: float = 1.0,
) -> bpy.types.Material:
    """Directional brushed PBR metal with micro-anisotropic roughness."""
    g = MatGraph(name)
    vec = g.mapped(brush_scale)
    grain = g.noise(vec, scale=1.5, detail=7.0, rough=0.7)
    g.link(g.mix_rgba(grain, color_a, color_b), g.bsdf.inputs["Base Color"])
    g.bsdf.inputs["Metallic"].default_value = metallic
    g.link(g.math("ADD", rough_base, g.math("MULTIPLY", grain, rough_span)), g.bsdf.inputs["Roughness"])
    g.bump(grain, distance=bump_dist, strength=0.22)
    return g.mat


def mat_brass(rough_base: float = 0.18) -> bpy.types.Material:
    return mat_brushed_metal(
        "brass",
        (0.76, 0.54, 0.18, 1.0),
        (0.92, 0.70, 0.28, 1.0),
        rough_base=rough_base,
        rough_span=0.09,
        brush_scale=(0.05, 0.85, 0.85),
    )


def mat_platinum() -> bpy.types.Material:
    return mat_brushed_metal(
        "platinum",
        (0.84, 0.87, 0.93, 1.0),
        (0.96, 0.97, 1.00, 1.0),
        rough_base=0.105,
        rough_span=0.055,
        brush_scale=(0.65, 0.65, 0.08),
        bump_dist=0.025,
    )


def mat_gold() -> bpy.types.Material:
    return mat_brushed_metal(
        "gold_24k",
        (0.90, 0.64, 0.14, 1.0),
        (1.00, 0.78, 0.25, 1.0),
        rough_base=0.12,
        rough_span=0.07,
        brush_scale=(0.55, 0.55, 0.07),
        bump_dist=0.03,
    )


def mat_silver() -> bpy.types.Material:
    return mat_brushed_metal(
        "sterling_silver",
        (0.74, 0.76, 0.80, 1.0),
        (0.89, 0.91, 0.94, 1.0),
        rough_base=0.17,
        rough_span=0.08,
        brush_scale=(0.60, 0.60, 0.07),
        bump_dist=0.035,
    )


def mat_bronze() -> bpy.types.Material:
    """Hand-rubbed aged bronze with darker crevice patina and warm copper highlights."""
    g = MatGraph("aged_bronze")
    ao = g.node("ShaderNodeAmbientOcclusion")
    ao.samples = 16
    ao.inputs["Distance"].default_value = 9.0
    grain = g.noise(g.mapped((0.45, 0.45, 0.08)), scale=1.2, detail=7.0, rough=0.68)
    patina_fac = g.math("MULTIPLY", ao.outputs["AO"], g.math("ADD", 0.75, g.math("MULTIPLY", grain, 0.25)))
    col = g.ramp(
        patina_fac,
        [
            (0.25, (0.20, 0.095, 0.045, 1.0)),
            (0.60, (0.56, 0.28, 0.13, 1.0)),
            (0.92, (0.82, 0.45, 0.22, 1.0)),
        ],
    )
    g.link(col, g.bsdf.inputs["Base Color"])
    g.bsdf.inputs["Metallic"].default_value = 0.95
    g.link(g.math("SUBTRACT", 0.40, g.math("MULTIPLY", patina_fac, 0.15)), g.bsdf.inputs["Roughness"])
    g.bump(grain, distance=0.05, strength=0.25)
    return g.mat


def mat_sapphire() -> bpy.types.Material:
    """Faceted sapphire crystal with deep royal cobalt refraction."""
    g = MatGraph("sapphire")
    g.bsdf.inputs["Base Color"].default_value = (0.012, 0.072, 0.72, 1.0)
    g.bsdf.inputs["Metallic"].default_value = 0.0
    g.bsdf.inputs["Roughness"].default_value = 0.015
    g.bsdf.inputs["IOR"].default_value = 1.77
    g.bsdf.inputs["Transmission Weight"].default_value = 0.92
    g.bsdf.inputs["Coat Weight"].default_value = 0.45
    g.bsdf.inputs["Coat Roughness"].default_value = 0.02
    g.bsdf.inputs["Emission Color"].default_value = (0.018, 0.095, 0.85, 1.0)
    g.bsdf.inputs["Emission Strength"].default_value = 0.06
    return g.mat


def mat_marble_nero() -> bpy.types.Material:
    """Dark Nero Marquina marble with crisp pale-ivory mineral veins."""
    g = MatGraph("marble_nero")
    wave = g.node("ShaderNodeTexWave")
    wave.wave_type = "BANDS"
    wave.bands_direction = "DIAGONAL"
    wave.inputs["Scale"].default_value = 0.045
    wave.inputs["Distortion"].default_value = 9.0
    wave.inputs["Detail"].default_value = 8.0
    wave.inputs["Detail Roughness"].default_value = 0.65
    g.link(g.obj, wave.inputs["Vector"])
    col = g.ramp(
        wave.outputs["Fac"],
        [
            (0.00, (0.018, 0.019, 0.024, 1.0)),
            (0.84, (0.028, 0.030, 0.036, 1.0)),
            (0.93, (0.38, 0.36, 0.34, 1.0)),
            (0.98, (0.78, 0.75, 0.70, 1.0)),
        ],
    )
    g.link(col, g.bsdf.inputs["Base Color"])
    g.bsdf.inputs["Roughness"].default_value = 0.16
    g.bsdf.inputs["Coat Weight"].default_value = 0.55
    g.bsdf.inputs["Coat Roughness"].default_value = 0.08
    return g.mat


def mat_slate() -> bpy.types.Material:
    """Dark architectural slate stone with fine mineral grain."""
    g = MatGraph("slate")
    n = g.noise(g.mapped((0.12, 0.12, 0.35)), scale=0.8, detail=8.0, rough=0.72)
    col = g.ramp(
        n,
        [
            (0.20, (0.045, 0.048, 0.056, 1.0)),
            (0.60, (0.082, 0.088, 0.100, 1.0)),
            (0.90, (0.120, 0.126, 0.140, 1.0)),
        ],
    )
    g.link(col, g.bsdf.inputs["Base Color"])
    g.link(g.math("ADD", 0.48, g.math("MULTIPLY", n, 0.16)), g.bsdf.inputs["Roughness"])
    g.bump(n, distance=0.12, strength=0.35)
    return g.mat


def mat_iron_locked() -> bpy.types.Material:
    """Dark smoked cast iron for the locked trophy silhouette."""
    g = MatGraph("iron_locked")
    n = g.noise(g.obj, scale=0.42, detail=7.0, rough=0.75)
    col = g.ramp(
        n,
        [
            (0.20, (0.026, 0.028, 0.034, 1.0)),
            (0.75, (0.052, 0.056, 0.068, 1.0)),
        ],
    )
    g.link(col, g.bsdf.inputs["Base Color"])
    g.bsdf.inputs["Metallic"].default_value = 0.65
    g.link(g.math("ADD", 0.34, g.math("MULTIPLY", n, 0.14)), g.bsdf.inputs["Roughness"])
    g.bump(n, distance=0.10, strength=0.30)
    return g.mat


def mat_matte_black(name: str = "matte_black", val: float = 0.014) -> bpy.types.Material:
    g = MatGraph(name)
    g.bsdf.inputs["Base Color"].default_value = (val, val, val * 1.08, 1.0)
    g.bsdf.inputs["Roughness"].default_value = 0.92
    g.bsdf.inputs["Specular IOR Level"].default_value = 0.08
    return g.mat


# ---------------------------------------------------------------------------
# Mesh & Curve Construction Helpers
# ---------------------------------------------------------------------------


def link_obj(name: str, mesh: bpy.types.Mesh, mat: bpy.types.Material | None = None) -> bpy.types.Object:
    obj = bpy.data.objects.new(name, mesh)
    if mat is not None:
        obj.data.materials.append(mat)
    bpy.context.collection.objects.link(obj)
    return obj


def shade_smooth(obj: bpy.types.Object) -> None:
    for poly in obj.data.polygons:
        poly.use_smooth = True


def add_bevel(obj: bpy.types.Object, width: float = 0.6, segments: int = 3, angle_deg: float = 35.0) -> None:
    mod = obj.modifiers.new("bevel", "BEVEL")
    mod.width = width
    mod.segments = segments
    mod.limit_method = "ANGLE"
    mod.angle_limit = math.radians(angle_deg)
    mod.harden_normals = False


def bevel_box(
    name: str,
    size: tuple[float, float, float],
    center: tuple[float, float, float],
    mat: bpy.types.Material,
    bevel: float = 0.6,
    segments: int = 3,
) -> bpy.types.Object:
    sx, sy, sz = size
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    bmesh.ops.scale(bm, vec=(sx, sy, sz), verts=bm.verts[:])
    bmesh.ops.translate(bm, vec=center, verts=bm.verts[:])
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    obj = link_obj(name, mesh, mat)
    if bevel > 0:
        add_bevel(obj, width=bevel, segments=segments)
    return obj


def lathe_profile(
    name: str,
    profile_rz: list[tuple[float, float]],
    mat: bpy.types.Material,
    segments: int = 96,
    flutes: int = 0,
    flute_depth: float = 0.0,
    flute_z: tuple[float, float] = (0.0, 0.0),
) -> bpy.types.Object:
    """Revolve a 2D (radius, z) profile around Z, optionally adding radial fluting."""
    bm = bmesh.new()
    ring_verts: list[list[bmesh.types.BMVert]] = []
    fz0, fz1 = flute_z
    for s in range(segments):
        ang = 2.0 * math.pi * s / segments
        ca, sa = math.cos(ang), math.sin(ang)
        col = []
        for r, z in profile_rz:
            rr = r
            if flutes > 0 and fz0 <= z <= fz1 and r > 1.0:
                t = math.sin(math.pi * (z - fz0) / max(fz1 - fz0, 1e-4))
                rr -= flute_depth * (t ** 0.7) * (0.5 - 0.5 * math.cos(flutes * ang))
            col.append(bm.verts.new((rr * ca, rr * sa, z)))
        ring_verts.append(col)

    for s in range(segments):
        s_next = (s + 1) % segments
        for i in range(len(profile_rz) - 1):
            v00 = ring_verts[s][i]
            v10 = ring_verts[s_next][i]
            v11 = ring_verts[s_next][i + 1]
            v01 = ring_verts[s][i + 1]
            if (v00.co - v10.co).length < 1e-4 and (v01.co - v11.co).length < 1e-4:
                continue
            if (v00.co - v10.co).length < 1e-4:
                bm.faces.new((v00, v11, v01))
            elif (v01.co - v11.co).length < 1e-4:
                bm.faces.new((v00, v10, v11))
            else:
                bm.faces.new((v00, v10, v11, v01))

    # Cap bottom/top if ends don't start at r=0
    if profile_rz[0][0] > 1e-3:
        bm.faces.new([ring_verts[s][0] for s in range(segments - 1, -1, -1)])
    if profile_rz[-1][0] > 1e-3:
        bm.faces.new([ring_verts[s][-1] for s in range(segments)])

    bmesh.ops.remove_doubles(bm, verts=bm.verts[:], dist=1e-4)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    obj = link_obj(name, mesh, mat)
    shade_smooth(obj)
    return obj


def plinth_tiered(
    name: str,
    tiers: list[tuple[float, float, float, bpy.types.Material, float]],
    sides: int = 8,
    xy_ratio: float = 0.85,
) -> list[bpy.types.Object]:
    """Build a multi-tier beveled plinth. Each tier: (w_bottom, w_top, z_height, mat, bevel)."""
    objs = []
    z_cur = 0.0
    for idx, (wb, wt, h, mat, bev) in enumerate(tiers):
        bm = bmesh.new()
        if sides == 4:
            pts = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)]
        elif sides == 8:
            c = 0.28
            pts = [
                (-0.5 + c, -0.5), (0.5 - c, -0.5),
                (0.5, -0.5 + c), (0.5, 0.5 - c),
                (0.5 - c, 0.5), (-0.5 + c, 0.5),
                (-0.5, 0.5 - c), (-0.5, -0.5 + c),
            ]
        else:
            pts = [(0.5 * math.cos(2 * math.pi * k / sides), 0.5 * math.sin(2 * math.pi * k / sides)) for k in range(sides)]

        bot = [bm.verts.new((px * wb, py * wb * xy_ratio, z_cur)) for px, py in pts]
        top = [bm.verts.new((px * wt, py * wt * xy_ratio, z_cur + h)) for px, py in pts]
        n = len(pts)
        for k in range(n):
            kn = (k + 1) % n
            bm.faces.new((bot[k], bot[kn], top[kn], top[k]))
        bm.faces.new(list(reversed(bot)))
        bm.faces.new(top)
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
        mesh = bpy.data.meshes.new(f"{name}_{idx}")
        bm.to_mesh(mesh)
        bm.free()
        obj = link_obj(f"{name}_{idx}", mesh, mat)
        if bev > 0:
            add_bevel(obj, width=bev, segments=3)
        objs.append(obj)
        z_cur += h
    return objs


def curved_handle(
    name: str,
    pts_with_radius: list[tuple[float, float, float, float]],
    mat: bpy.types.Material,
    mirror_x: bool = True,
) -> list[bpy.types.Object]:
    """Smooth 3D curved handle with variable radius, optionally mirrored across X."""
    signs = (-1.0, 1.0) if mirror_x else (1.0,)
    objs = []
    for sgn in signs:
        curve = bpy.data.curves.new(f"{name}_{'R' if sgn > 0 else 'L'}", "CURVE")
        curve.dimensions = "3D"
        curve.bevel_depth = 1.0
        curve.bevel_resolution = 8
        curve.use_fill_caps = True
        spline = curve.splines.new("NURBS")
        spline.points.add(len(pts_with_radius) - 1)
        for pt, (x, y, z, r) in zip(spline.points, pts_with_radius):
            pt.co = (x * sgn, y, z, 1.0)
            pt.radius = r
        spline.use_endpoint_u = True
        spline.order_u = min(4, len(pts_with_radius))
        spline.resolution_u = 24
        curve.materials.append(mat)
        obj = bpy.data.objects.new(curve.name, curve)
        bpy.context.collection.objects.link(obj)
        objs.append(obj)
    return objs


def gem_faceted(
    name: str,
    radius: float,
    depth: float,
    center: tuple[float, float, float],
    mat: bpy.types.Material,
) -> bpy.types.Object:
    """Brilliant/cushion-cut faceted gem pointing toward -Y (toward camera)."""
    bm = bmesh.new()
    sides = 8
    table_r = radius * 0.56
    girdle_y = 0.0
    table_y = -depth * 0.38
    culet_y = depth * 0.62

    table_vs = []
    girdle_vs = []
    for k in range(sides):
        ang = 2 * math.pi * (k + 0.5) / sides
        table_vs.append(bm.verts.new((table_r * math.cos(ang), table_y, table_r * math.sin(ang))))
        girdle_vs.append(bm.verts.new((radius * math.cos(ang), girdle_y, radius * math.sin(ang))))
    culet_v = bm.verts.new((0.0, culet_y, 0.0))

    bm.faces.new(list(reversed(table_vs)))
    for k in range(sides):
        kn = (k + 1) % sides
        bm.faces.new((table_vs[k], table_vs[kn], girdle_vs[kn], girdle_vs[k]))
        bm.faces.new((girdle_vs[k], girdle_vs[kn], culet_v))

    bmesh.ops.translate(bm, vec=center, verts=bm.verts[:])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    return link_obj(name, mesh, mat)


def laurel_wreath(
    name: str,
    cx: float,
    cy: float,
    cz: float,
    radius: float,
    bowl_r: float,
    mat: bpy.types.Material,
) -> None:
    """3D sculpted twin laurel branches curving cleanly across the front of a chalice bowl."""
    for sgn in (-1.0, 1.0):
        # Stem arc
        stem_pts = []
        for i in range(9):
            t = i / 8.0
            ang = -0.35 + t * 1.95
            px = sgn * radius * math.sin(ang) * 0.92
            pz = cz - radius * math.cos(ang) * 0.88
            # Wrap smoothly around the chalice surface
            py = -math.sqrt(max(bowl_r ** 2 - px ** 2, 1.0)) + 0.3
            stem_pts.append((px, py, pz, 0.72 - 0.25 * t))
        curved_handle(f"{name}_stem_{'R' if sgn > 0 else 'L'}", stem_pts, mat, mirror_x=False)

        # Leaves along stem
        for i in range(1, 8):
            t = i / 8.0
            ang = -0.35 + t * 1.95
            bx = sgn * radius * math.sin(ang) * 0.92
            bz = cz - radius * math.cos(ang) * 0.88
            by = -math.sqrt(max(bowl_r ** 2 - bx ** 2, 1.0)) - 0.35
            for side in (-1.0, 1.0):
                bm = bmesh.new()
                bmesh.ops.create_icosphere(bm, subdivisions=2, radius=1.0)
                bmesh.ops.scale(bm, vec=(1.2, 0.45, 2.7), verts=bm.verts[:])
                rot = Matrix.Rotation(sgn * (ang * 0.65 + side * 0.55), 4, "Y")
                bmesh.ops.rotate(bm, matrix=rot, verts=bm.verts[:])
                bmesh.ops.translate(bm, vec=(bx + sgn * side * 1.4, by, bz + 0.8), verts=bm.verts[:])
                mesh = bpy.data.meshes.new(f"{name}_leaf_{sgn}_{i}_{side}")
                bm.to_mesh(mesh)
                bm.free()
                obj = link_obj(mesh.name, mesh, mat)
                shade_smooth(obj)


def star_medallion(
    name: str,
    center: tuple[float, float, float],
    outer_r: float,
    inner_r: float,
    depth: float,
    mat: bpy.types.Material,
) -> bpy.types.Object:
    """Raised beveled 5-pointed star inside a circular bezel ring facing -Y."""
    cx, cy, cz = center
    bm = bmesh.new()
    front_pts = []
    for k in range(10):
        ang = math.pi / 2 + k * math.pi / 5
        r = outer_r if k % 2 == 0 else inner_r
        front_pts.append((cx + r * math.cos(ang), cy - depth, cz + r * math.sin(ang)))
    back_vs = [bm.verts.new((x, cy + 1.5, z)) for (x, _, z) in front_pts]
    mid_vs = [bm.verts.new((x, y + depth * 0.45, z)) for (x, y, z) in front_pts]
    peak_v = bm.verts.new((cx, cy - depth * 1.15, cz))

    n = len(front_pts)
    for k in range(n):
        kn = (k + 1) % n
        bm.faces.new((back_vs[k], back_vs[kn], mid_vs[kn], mid_vs[k]))
        bm.faces.new((mid_vs[k], mid_vs[kn], peak_v))
    bm.faces.new(list(reversed(back_vs)))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    obj = link_obj(name, mesh, mat)
    add_bevel(obj, width=0.35, segments=2)
    return obj


def rosette_medallion(
    name: str,
    center: tuple[float, float, float],
    radius: float,
    petals: int,
    mat: bpy.types.Material,
) -> bpy.types.Object:
    """Classical sculpted brass rosette medallion facing -Y for the cabinet pediment."""
    cx, cy, cz = center
    bm = bmesh.new()
    # Outer ring
    for p in range(petals):
        ang = 2 * math.pi * p / petals
        bmesh.ops.create_icosphere(
            bm,
            subdivisions=2,
            radius=radius * 0.22,
            matrix=Matrix.Translation((cx + radius * 0.52 * math.cos(ang), cy - 1.2, cz + radius * 0.52 * math.sin(ang)))
            @ Matrix.Rotation(ang, 4, "Y")
            @ Matrix.Scale(1.7, 4, (1, 0, 0))
            @ Matrix.Scale(0.55, 4, (0, 1, 0)),
        )
    bmesh.ops.create_uvsphere(
        bm,
        u_segments=24,
        v_segments=12,
        radius=radius * 0.28,
        matrix=Matrix.Translation((cx, cy - 1.6, cz)) @ Matrix.Scale(0.6, 4, (0, 1, 0)),
    )
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    obj = link_obj(name, mesh, mat)
    shade_smooth(obj)
    return obj


# ---------------------------------------------------------------------------
# Camera & Studio Lighting
# ---------------------------------------------------------------------------


def setup_cycles(res_x: int, res_y: int, samples: int, transparent: bool) -> bpy.types.Scene:
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = samples
    scene.cycles.use_adaptive_sampling = True
    scene.cycles.adaptive_threshold = 0.01
    scene.cycles.sample_clamp_direct = 8.0
    scene.cycles.sample_clamp_indirect = 4.0
    scene.cycles.max_bounces = 8
    scene.cycles.glossy_bounces = 5
    scene.cycles.transmission_bounces = 8
    scene.cycles.use_denoising = False
    scene.render.film_transparent = transparent
    scene.view_settings.view_transform = "AgX"
    scene.view_settings.look = "AgX - Medium High Contrast"
    scene.view_settings.exposure = 0.0
    scene.render.resolution_x = res_x
    scene.render.resolution_y = res_y
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"
    return scene


def add_area_light(
    name: str,
    loc: tuple[float, float, float],
    target: tuple[float, float, float],
    size_x: float,
    size_y: float | None,
    irradiance: float,
    color: tuple[float, float, float] = (1.0, 1.0, 1.0),
    spread_deg: float = 160.0,
    cast_shadow: bool = True,
) -> bpy.types.Object:
    light_data = bpy.data.lights.new(name, "AREA")
    if size_y is None:
        light_data.shape = "SQUARE"
        light_data.size = size_x
    else:
        light_data.shape = "RECTANGLE"
        light_data.size = size_x
        light_data.size_y = size_y
    light_data.color = color
    light_data.spread = math.radians(spread_deg)
    light_data.use_shadow = cast_shadow
    vec = Vector(target) - Vector(loc)
    dist = max(vec.length, 1.0)
    light_data.energy = irradiance * math.pi * (dist ** 2)
    obj = bpy.data.objects.new(name, light_data)
    obj.location = loc
    obj.rotation_euler = (-vec).to_track_quat("Z", "Y").to_euler()
    bpy.context.collection.objects.link(obj)
    return obj


def setup_studio_world(scene: bpy.types.Scene) -> None:
    """Smooth gradient studio softbox dome for silky glossy reflections without hard card seams."""
    world = bpy.data.worlds.new("world")
    world.use_nodes = True
    nt = world.node_tree
    nt.nodes.clear()

    out = nt.nodes.new("ShaderNodeOutputWorld")
    lp = nt.nodes.new("ShaderNodeLightPath")
    mix_sh = nt.nodes.new("ShaderNodeMixShader")
    bg_diff = nt.nodes.new("ShaderNodeBackground")
    bg_diff.inputs["Color"].default_value = (0.16, 0.15, 0.14, 1.0)
    bg_diff.inputs["Strength"].default_value = 0.30

    bg_gloss = nt.nodes.new("ShaderNodeBackground")
    bg_gloss.inputs["Strength"].default_value = 1.0

    # Smooth vertical + directional studio softbox gradient using Generated ray vector
    tc = nt.nodes.new("ShaderNodeTexCoord")
    sep = nt.nodes.new("ShaderNodeSeparateXYZ")
    nt.links.new(tc.outputs["Generated"], sep.inputs["Vector"])

    # Combine horizontal softbox ribbon (bright left/right lobes, warm top, subtle dark center band)
    # Map X in [-1, 1] -> smooth softbox luminance
    ramp_x = nt.nodes.new("ShaderNodeValToRGB")
    map_x = nt.nodes.new("ShaderNodeMath")
    map_x.operation = "MULTIPLY_ADD"
    map_x.inputs[1].default_value = 0.5
    map_x.inputs[2].default_value = 0.5
    nt.links.new(sep.outputs["X"], map_x.inputs[0])
    nt.links.new(map_x.outputs[0], ramp_x.inputs["Factor"])

    elts = ramp_x.color_ramp.elements
    while len(elts) < 5:
        elts.new(0.5)
    elts[0].position, elts[0].color = 0.08, (1.35, 1.28, 1.18, 1.0)
    elts[1].position, elts[1].color = 0.32, (0.38, 0.38, 0.42, 1.0)
    elts[2].position, elts[2].color = 0.48, (0.16, 0.16, 0.19, 1.0)
    elts[3].position, elts[3].color = 0.68, (0.65, 0.70, 0.80, 1.0)
    elts[4].position, elts[4].color = 0.92, (1.05, 1.12, 1.25, 1.0)

    # Darken lower hemisphere gently (warm walnut shelf bounce below horizon Z < 0)
    ramp_z = nt.nodes.new("ShaderNodeValToRGB")
    map_z = nt.nodes.new("ShaderNodeMath")
    map_z.operation = "MULTIPLY_ADD"
    map_z.inputs[1].default_value = 0.5
    map_z.inputs[2].default_value = 0.5
    nt.links.new(sep.outputs["Z"], map_z.inputs[0])
    nt.links.new(map_z.outputs[0], ramp_z.inputs["Factor"])
    zelts = ramp_z.color_ramp.elements
    while len(zelts) < 3:
        zelts.new(0.5)
    zelts[0].position, zelts[0].color = 0.25, (0.14, 0.145, 0.16, 1.0)
    zelts[1].position, zelts[1].color = 0.52, (0.85, 0.86, 0.90, 1.0)
    zelts[2].position, zelts[2].color = 0.85, (1.15, 1.14, 1.12, 1.0)

    mul = nt.nodes.new("ShaderNodeMix")
    mul.data_type = "RGBA"
    mul.blend_type = "MULTIPLY"
    mul.inputs[0].default_value = 1.0
    rgba_ins = [s for s in mul.inputs if s.type == "RGBA"]
    rgba_out = [s for s in mul.outputs if s.type == "RGBA"][0]
    nt.links.new(ramp_x.outputs["Color"], rgba_ins[0])
    nt.links.new(ramp_z.outputs["Color"], rgba_ins[1])

    nt.links.new(rgba_out, bg_gloss.inputs["Color"])
    nt.links.new(lp.outputs["Is Glossy Ray"], mix_sh.inputs["Fac"])
    nt.links.new(bg_diff.outputs["Background"], mix_sh.inputs[1])
    nt.links.new(bg_gloss.outputs["Background"], mix_sh.inputs[2])
    nt.links.new(mix_sh.outputs["Shader"], out.inputs["Surface"])
    scene.world = world


def setup_sprite_studio(res_x: int, res_y: int, cam_loc: tuple[float, float, float], cam_rot_x_deg: float, shift_y: float = 0.0) -> bpy.types.Scene:
    scene = setup_cycles(res_x, res_y, SAMPLES_SPRITE, transparent=True)
    cam_data = bpy.data.cameras.new("camera")
    cam_data.type = "PERSP"
    cam_data.sensor_fit = "HORIZONTAL"
    cam_data.sensor_width = 36.0
    cam_data.lens = 85.0
    cam_data.shift_y = shift_y
    cam_data.clip_end = 5000.0
    cam = bpy.data.objects.new("camera", cam_data)
    cam.location = cam_loc
    cam.rotation_euler = (math.radians(cam_rot_x_deg), 0.0, 0.0)
    bpy.context.collection.objects.link(cam)
    scene.camera = cam

    setup_studio_world(scene)

    # Invisible-to-camera subtle neutral slate bounce disc at Z = 0 for authentic lower-bevel reflections
    bm = bmesh.new()
    bmesh.ops.create_circle(bm, cap_ends=True, cap_tris=False, radius=42.0, segments=48)
    mesh = bpy.data.meshes.new("shelf_bounce")
    bm.to_mesh(mesh)
    bm.free()
    bmat = bpy.data.materials.new("walnut_bounce_mat")
    bmat.use_nodes = True
    bbsdf = bmat.node_tree.nodes["Principled BSDF"]
    bbsdf.inputs["Base Color"].default_value = (0.045, 0.046, 0.052, 1.0)
    bbsdf.inputs["Roughness"].default_value = 0.60
    bounce = link_obj("shelf_bounce", mesh, bmat)
    bounce.visible_camera = False
    bounce.visible_shadow = False

    # Studio lighting (key, soft fill, rim, top sheen) — clean grounded shadow added cleanly in post
    add_area_light("top_softbox", (0.0, -18.0, 145.0), (0.0, 0.0, 45.0), 85.0, 65.0, 1.45, (1.0, 0.96, 0.90), spread_deg=110.0)
    add_area_light("key_left", (-85.0, -135.0, 140.0), (0.0, 0.0, 52.0), 95.0, 120.0, 2.25, (1.0, 0.93, 0.85), spread_deg=120.0)
    add_area_light("fill_right", (110.0, -135.0, 72.0), (0.0, 0.0, 48.0), 115.0, 120.0, 0.82, (0.84, 0.90, 1.0), spread_deg=140.0, cast_shadow=False)
    add_area_light("rim_right", (78.0, 92.0, 108.0), (0.0, 0.0, 55.0), 65.0, 120.0, 1.85, (1.0, 0.96, 0.90), spread_deg=110.0, cast_shadow=False)
    add_area_light("rim_left", (-82.0, 88.0, 96.0), (0.0, 0.0, 55.0), 55.0, 110.0, 1.15, (0.86, 0.92, 1.0), spread_deg=110.0, cast_shadow=False)
    return scene


# ---------------------------------------------------------------------------
# Trophy Room Scene (720x720)
# ---------------------------------------------------------------------------


def build_trophy_room() -> tuple[bpy.types.Scene, bpy.types.Object, dict[str, float]]:
    """Build the 720x720 Walnut & Brass RetroAchievements Trophy Cabinet."""
    # World convention: 1 unit = 1 mm at front plane Y = 0.
    # px = X + 360, py = 360 - Z.
    # Shift-lens perspective camera at Z_cam = 360 (py = 0), Y = -1300 (65mm lens / 36mm sensor),
    # optical axis horizontal (+Y), so verticals stay strictly vertical while shelves expose depth.
    scene = setup_cycles(720, 720, SAMPLES_ROOM, transparent=False)

    cam_data = bpy.data.cameras.new("camera")
    cam_data.type = "PERSP"
    cam_data.sensor_fit = "HORIZONTAL"
    cam_data.sensor_width = 36.0
    cam_data.lens = 65.0
    cam_z = 360.0
    cam_dist = 720.0 * (cam_data.lens / cam_data.sensor_width)  # 1300.0
    cam_data.shift_y = -cam_z / 720.0  # -0.5 -> Z=0 centered at py=360
    cam_data.clip_end = 5000.0
    cam = bpy.data.objects.new("camera", cam_data)
    cam.location = (0.0, -cam_dist, cam_z)
    cam.rotation_euler = (math.pi / 2.0, 0.0, 0.0)
    bpy.context.collection.objects.link(cam)
    scene.camera = cam

    world = bpy.data.worlds.new("world")
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs["Color"].default_value = (0.28, 0.19, 0.14, 1.0)
    bg.inputs["Strength"].default_value = 0.06
    scene.world = world

    walnut_x = mat_walnut("walnut_x", axis="X")
    walnut_z = mat_walnut("walnut_z", axis="Z")
    walnut_dark = mat_walnut("walnut_dark", axis="X", dark_bias=0.75)
    velvet = mat_velvet()
    brass = mat_brass(rough_base=0.20)

    def pz(py: float) -> float:
        return 360.0 - py

    # 1. Full outer dark oak/walnut room wainscot backdrop placed behind the cabinet alcove (Y=116)
    bevel_box("room_wall_back", (860.0, 12.0, 860.0), (0.0, 116.0, 0.0), walnut_z, bevel=0.0)

    # Outer flanking wall pilasters outside 640x480 (px < 42, px > 678)
    for sx in (-1.0, 1.0):
        bevel_box(f"outer_wall_{sx}", (60.0, 18.0, 760.0), (sx * 348.0, 14.0, 0.0), walnut_z, bevel=0.8)
        # Subtle recessed outer dado stiles
        bevel_box(f"outer_stile_{sx}", (14.0, 10.0, 740.0), (sx * 326.0, 6.0, 0.0), walnut_z, bevel=0.6)

    # 2. Cabinet interior alcove (velvet back & walnut side walls)
    # Interior opening spans X in [-294, +294] (px 66..654) at front Y=0, depth Y=10..100
    z_alcove_top = pz(160.0)    # +200
    z_alcove_bot = pz(468.0)    # -108
    alcove_h = z_alcove_top - z_alcove_bot
    alcove_zc = 0.5 * (z_alcove_top + z_alcove_bot)
    bevel_box("velvet_back", (604.0, 10.0, alcove_h + 10.0), (0.0, 102.0, alcove_zc), velvet, bevel=0.0)

    # Subtle upholstered vertical velvet dividers hinting at 4 bays per shelf (very low relief)
    for bx in (-142.0, 0.0, 142.0):
        bevel_box(f"velvet_seam_{bx}", (2.2, 3.0, alcove_h), (bx, 97.5, alcove_zc), walnut_dark, bevel=0.5)

    # Cabinet left & right interior side linings
    for sx in (-1.0, 1.0):
        bevel_box(f"cabinet_inner_wall_{sx}", (14.0, 106.0, alcove_h + 20.0), (sx * 301.0, 55.0, alcove_zc), walnut_z, bevel=0.5)

    # 3. Left & Right fluted walnut pilasters (inside X in [-318, -294] -> px 42..66 and px 654..678)
    # Strictly inside 640x480 crop (px 40..680)!
    z_pil_top = pz(124.0)
    z_pil_bot = pz(596.0)
    pil_h = z_pil_top - z_pil_bot
    pil_zc = 0.5 * (z_pil_top + z_pil_bot)
    for sx in (-1.0, 1.0):
        px_center = sx * 306.0  # spans 294..318 (24mm wide)
        bevel_box(f"pilaster_body_{sx}", (24.0, 18.0, pil_h), (px_center, 0.0, pil_zc), walnut_z, bevel=0.9)
        # Dual vertical carved flute strips on front face
        for fx in (-4.8, 4.8):
            bevel_box(f"pilaster_flute_{sx}_{fx}", (3.2, 4.0, pil_h - 48.0), (px_center + fx, -8.5, pil_zc), walnut_dark, bevel=0.7)
        # Brass capital & base collar bands + shelf level collars
        for cy, ch in ((pz(161.0), 4.5), (pz(298.0), 3.5), (pz(457.0), 3.5), (pz(582.0), 4.5)):
            bevel_box(f"pilaster_brass_{sx}_{cy}", (26.5, 19.5, ch), (px_center, -0.5, cy), brass, bevel=0.5)

    # 4. Header Architrave Zone (py = 124..162) — top of 720x480 & 640x480 viewports
    bevel_box("header_frame", (636.0, 22.0, 38.0), (0.0, -2.0, pz(143.0)), walnut_x, bevel=1.0)
    # Recessed dark walnut header plate with thin brushed-brass perimeter trim (py 130..156, px 76..644)
    bevel_box("header_brass_trim", (568.0, 6.0, 26.0), (0.0, -10.5, pz(143.0)), brass, bevel=0.5)
    bevel_box("header_inlay", (561.0, 6.0, 21.0), (0.0, -11.5, pz(143.0)), walnut_dark, bevel=0.5)
    # Overhanging architrave soffit baffle hiding upper cove lights
    bevel_box("header_baffle", (600.0, 26.0, 6.0), (0.0, 16.0, pz(161.0)), walnut_x, bevel=0.4)

    # 5. Upper & Lower Display Shelf Planks (spanning px ~ 64..656, Y = 10..100)
    z_top_surface = pz(294.0)   # +66.0 -> front lip at Y=10 projects to py ~ 291
    z_bot_surface = pz(453.0)   # -93.0 -> front lip at Y=10 projects to py ~ 449
    for tag, z_top, thick in (("top_shelf", z_top_surface, 15.0), ("bot_shelf", z_bot_surface, 16.0)):
        zc = z_top - thick / 2.0
        # Main shelf slab (from Y=10.5 to Y=100)
        bevel_box(f"{tag}_plank", (594.0, 89.5, thick), (0.0, 55.25, zc), walnut_x, bevel=0.9)
        # Molded front bullnose fascia lip (Y=7..12) with inlaid brass edge strip
        bevel_box(f"{tag}_nosing", (592.0, 6.0, thick), (0.0, 10.0, zc), walnut_x, bevel=1.2)
        bevel_box(f"{tag}_brass_strip", (590.0, 2.4, 2.6), (0.0, 6.6, zc - 0.5), brass, bevel=0.35)
        # Hidden light valence lip under top shelf
        if tag == "top_shelf":
            bevel_box("top_shelf_baffle", (590.0, 14.0, 5.0), (0.0, 16.0, z_top - thick - 1.5), walnut_x, bevel=0.3)

    # 6. Lower Brass Nameplate / Inspection Ledge Panel (py = 468..598)
    # Shadowed transition molding below bottom shelf
    bevel_box("apron_cove", (600.0, 18.0, 14.0), (0.0, 6.0, pz(472.0)), walnut_x, bevel=0.9)
    # Backing walnut panel spanning py 476..584
    bevel_box("plaque_back_wall", (600.0, 14.0, 108.0), (0.0, 12.0, pz(530.0)), walnut_x, bevel=0.6)
    # Framed dark walnut & brass detail plaque backdrop (py = 485..565, px = 78..642)
    bevel_box("plaque_outer_molding", (568.0, 10.0, 82.0), (0.0, 4.0, pz(525.0)), walnut_x, bevel=1.0)
    bevel_box("plaque_brass_frame", (556.0, 6.0, 72.0), (0.0, 1.5, pz(525.0)), brass, bevel=0.55)
    bevel_box("plaque_dark_panel", (549.0, 6.0, 65.5), (0.0, 0.5, pz(525.0)), walnut_dark, bevel=0.5)
    # Corner brass stud accents on plaque frame
    for sx in (-1.0, 1.0):
        for sz in (-1.0, 1.0):
            bevel_box(f"plaque_stud_{sx}_{sz}", (3.2, 2.5, 3.2), (sx * 268.0, -2.5, pz(525.0) + sz * 29.5), brass, bevel=0.4)
    # Cabinet base sill / dado rail right at py = 582..600 (cleanly finishes 720x480 / 640x480 bottom)
    bevel_box("cabinet_base_sill", (650.0, 26.0, 16.0), (0.0, -3.0, pz(590.0)), walnut_x, bevel=1.1)
    bevel_box("cabinet_sill_brass", (646.0, 4.0, 2.2), (0.0, -15.5, pz(586.0)), brass, bevel=0.3)

    # 7. Upper Pediment & Crown Cornice (py = 0..124, visible in 720x720 RG CubeXX)
    # Recessed museum ceiling wall & crown moldings
    bevel_box("pediment_frieze_bg", (648.0, 18.0, 88.0), (0.0, 6.0, pz(78.0)), walnut_x, bevel=0.6)
    # Stepped overhanging crown cornice (py 20..44)
    bevel_box("cornice_step_top", (672.0, 32.0, 12.0), (0.0, -8.0, pz(24.0)), walnut_x, bevel=1.2)
    bevel_box("cornice_step_mid", (658.0, 26.0, 11.0), (0.0, -5.0, pz(35.0)), walnut_x, bevel=1.0)
    bevel_box("cornice_brass_fillet", (648.0, 6.0, 2.5), (0.0, -16.0, pz(41.0)), brass, bevel=0.35)
    bevel_box("cornice_bed_molding", (644.0, 20.0, 10.0), (0.0, -2.0, pz(47.0)), walnut_x, bevel=0.9)
    # Flanking recessed walnut frieze coffers & center crest panel
    for sx in (-1.0, 1.0):
        bevel_box(f"frieze_coffer_frame_{sx}", (176.0, 8.0, 44.0), (sx * 196.0, -4.0, pz(82.0)), walnut_x, bevel=0.8)
        bevel_box(f"frieze_coffer_inlay_{sx}", (162.0, 6.0, 32.0), (sx * 196.0, -1.5, pz(82.0)), walnut_dark, bevel=0.5)
    # Center sculpted crest plaque + brass rosette medallion
    bevel_box("crest_plinth", (170.0, 12.0, 50.0), (0.0, -6.0, pz(80.0)), walnut_x, bevel=1.1)
    bevel_box("crest_brass_border", (152.0, 6.0, 38.0), (0.0, -10.5, pz(80.0)), brass, bevel=0.5)
    bevel_box("crest_center_panel", (144.0, 6.0, 31.0), (0.0, -11.5, pz(80.0)), walnut_dark, bevel=0.4)
    rosette_medallion("crest_rosette", (0.0, -14.2, pz(80.0)), radius=14.0, petals=12, mat=brass)
    # Lower cornice bead ending right at py = 124
    bevel_box("pediment_lower_bead", (648.0, 22.0, 14.0), (0.0, -3.0, pz(117.0)), walnut_x, bevel=1.0)

    # 8. Lower Paneled Walnut Wainscoting (py = 600..720, visible in 720x720 RG CubeXX)
    bevel_box("wainscot_backing", (648.0, 16.0, 124.0), (0.0, 10.0, pz(662.0)), walnut_z, bevel=0.4)
    # Matching vertical stiles aligning with pilasters + 2 interior bay dividers
    for wx in (-306.0, -102.0, 102.0, 306.0):
        bevel_box(f"wainscot_stile_{wx}", (24.0, 14.0, 114.0), (wx, 2.0, pz(656.0)), walnut_z, bevel=0.8)
    # Three raised beveled wainscot panels
    for cx in (-204.0, 0.0, 204.0):
        bevel_box(f"wainscot_panel_outer_{cx}", (156.0, 8.0, 76.0), (cx, 5.0, pz(652.0)), walnut_dark, bevel=0.8)
        bevel_box(f"wainscot_panel_raised_{cx}", (134.0, 10.0, 56.0), (cx, 1.5, pz(652.0)), walnut_x, bevel=1.2)
    # Baseboard plinth molding at bottom (py = 700..720)
    bevel_box("baseboard_trim", (740.0, 24.0, 24.0), (0.0, -1.0, pz(712.0)), walnut_x, bevel=1.0)

    # -----------------------------------------------------------------------
    # Warm Museum Spotlighting & Recessed Shelf LED Strips
    # -----------------------------------------------------------------------
    warm_led = (1.0, 0.74, 0.44)
    warm_museum = (1.0, 0.84, 0.62)

    # 4 recessed spotlights inside the upper shelf soffit (Y=42, Z=pz(164), hidden behind baffle)
    slot_xs = (-212.0, -70.6, 70.6, 212.0)
    for sx in slot_xs:
        add_area_light(f"top_spot_{sx}", (sx, 48.0, z_alcove_top - 4.0), (sx, 58.0, z_top_surface), 74.0, 38.0, 1.55, warm_led, spread_deg=85.0)
        add_area_light(f"bot_spot_{sx}", (sx, 48.0, z_top_surface - 17.5), (sx, 58.0, z_bot_surface), 74.0, 38.0, 1.65, warm_led, spread_deg=85.0)

    # Soft linear cove wash across upper & lower shelves
    add_area_light("top_cove_bar", (0.0, 32.0, z_alcove_top - 3.0), (0.0, 80.0, z_top_surface + 32.0), 540.0, 16.0, 0.42, warm_led, spread_deg=135.0)
    add_area_light("bot_cove_bar", (0.0, 32.0, z_top_surface - 17.0), (0.0, 80.0, z_bot_surface + 36.0), 540.0, 16.0, 0.45, warm_led, spread_deg=135.0)

    # Symmetrical warm gallery uplight/picture-light grazing the pediment cornice & brass crest
    add_area_light("pediment_light", (0.0, -190.0, pz(12.0)), (0.0, -4.0, pz(78.0)), 620.0, 56.0, 0.90, warm_museum, spread_deg=125.0)

    # Subtle grazing light for lower inspection plaque brass frame & wainscoting
    add_area_light("plaque_grazing", (0.0, -140.0, pz(470.0)), (0.0, 0.0, pz(530.0)), 480.0, 36.0, 0.55, warm_museum, spread_deg=120.0)
    add_area_light("wainscot_fill", (0.0, -220.0, pz(630.0)), (0.0, 0.0, pz(665.0)), 520.0, 60.0, 0.42, warm_museum, spread_deg=130.0)

    # Overall soft museum key from upper-left-front and gentle fill from lower-right
    add_area_light("room_key", (-420.0, -920.0, 380.0), (0.0, 0.0, 0.0), 600.0, 600.0, 0.92, warm_museum, spread_deg=140.0)
    add_area_light("room_fill", (480.0, -960.0, -140.0), (0.0, 0.0, 0.0), 700.0, 650.0, 0.34, (0.78, 0.84, 0.98), spread_deg=150.0)

    # Measure exact projected pixel positions via Blender's camera projection
    bpy.context.view_layer.update()

    def proj(pt: tuple[float, float, float]) -> tuple[float, float]:
        uvz = world_to_camera_view(scene, cam, Vector(pt))
        return uvz.x * 720.0, (1.0 - uvz.y) * 720.0

    # Key shelf and zone coordinates
    _, y_top_front = proj((0.0, 7.0, z_top_surface))
    _, y_top_rest = proj((0.0, 34.0, z_top_surface))
    _, y_top_back = proj((0.0, 100.0, z_top_surface))
    _, y_bot_front = proj((0.0, 7.0, z_bot_surface))
    _, y_bot_rest = proj((0.0, 34.0, z_bot_surface))
    _, y_bot_back = proj((0.0, 100.0, z_bot_surface))
    x_left_col, _ = proj((-294.0, 10.0, 0.0))
    x_right_col, _ = proj((294.0, 10.0, 0.0))
    x_back_left, _ = proj((-294.0, 100.0, 0.0))
    x_back_right, _ = proj((294.0, 100.0, 0.0))
    _, y_hdr_top = proj((0.0, -10.5, pz(130.0)))
    _, y_hdr_bot = proj((0.0, -10.5, pz(156.0)))
    _, y_plq_top = proj((0.0, 0.5, pz(485.0)))
    _, y_plq_bot = proj((0.0, 0.5, pz(565.0)))

    metrics = {
        "y_top_front": y_top_front,
        "y_top_rest": y_top_rest,
        "y_top_back": y_top_back,
        "y_bot_front": y_bot_front,
        "y_bot_rest": y_bot_rest,
        "y_bot_back": y_bot_back,
        "x_front_min": x_left_col,
        "x_front_max": x_right_col,
        "x_back_min": x_back_left,
        "x_back_max": x_back_right,
        "y_hdr_top": y_hdr_top,
        "y_hdr_bot": y_hdr_bot,
        "y_plq_top": y_plq_top,
        "y_plq_bot": y_plq_bot,
    }
    return scene, cam, metrics


# ---------------------------------------------------------------------------
# Individual 3D Trophy Sculptures (112x144 RGBA)
# ---------------------------------------------------------------------------


def build_chalice_platinum() -> None:
    marble = mat_marble_nero()
    plat = mat_platinum()
    sapphire = mat_sapphire()

    # Stepped Nero Marquina marble octagonal plinth + platinum collar
    plinth_tiered(
        "plat_plinth",
        [
            (48.0, 46.0, 7.5, marble, 0.65),
            (41.0, 39.0, 9.5, marble, 0.65),
            (34.0, 31.0, 3.0, plat, 0.45),
        ],
        sides=8,
        xy_ratio=0.82,
    )
    # Small engraved front nameplate bar on the marble plinth
    bevel_box("plat_plinth_plate", (24.0, 2.2, 5.2), (0.0, -16.8, 12.2), plat, bevel=0.45)

    # Tall regal fluted chalice & baluster stem (Z = 20..117)
    profile = [
        (0.0, 19.5),
        (16.0, 19.5),
        (16.5, 22.5),
        (11.0, 25.5),
        (7.5, 31.0),
        (10.5, 37.0),   # stem knop
        (6.8, 43.0),
        (6.2, 51.0),
        (10.0, 56.0),   # bowl collar
        (17.5, 63.0),
        (23.5, 76.0),
        (26.5, 92.0),
        (29.0, 108.0),
        (31.2, 114.5),  # rim outer lip
        (30.0, 116.5),
        (27.5, 116.2),  # inner cup rim
        (24.5, 106.0),
        (20.0, 82.0),
        (12.0, 68.0),
        (0.0, 64.0),
    ]
    lathe_profile("plat_chalice", profile, plat, segments=112, flutes=16, flute_depth=0.95, flute_z=(59.0, 86.0))

    # Twin S-scroll handles curling gracefully from lower bowl to upper rim
    handle_pts = [
        (20.0, 0.0, 64.0, 2.2),
        (30.5, 0.0, 69.0, 2.5),
        (38.0, 0.0, 84.0, 2.7),
        (39.5, 0.0, 102.0, 2.4),
        (34.0, 0.0, 111.5, 2.0),
        (27.8, 0.0, 107.0, 1.8),
    ]
    curved_handle("plat_handles", handle_pts, plat, mirror_x=True)

    # Sapphire crystal gem inlay inside an octagonal beveled platinum bezel on the front bowl belly
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=8, radius1=8.2, radius2=7.0, depth=4.4)
    bmesh.ops.rotate(bm, matrix=Matrix.Rotation(math.pi / 8.0, 4, "Y") @ Matrix.Rotation(math.pi / 2.0, 4, "X"), verts=bm.verts[:])
    bmesh.ops.translate(bm, vec=(0.0, -24.8, 86.0), verts=bm.verts[:])
    mesh = bpy.data.meshes.new("plat_gem_bezel")
    bm.to_mesh(mesh)
    bm.free()
    bezel = link_obj("plat_gem_bezel", mesh, plat)
    add_bevel(bezel, width=0.6, segments=2)
    gem_faceted("plat_sapphire", radius=5.4, depth=4.0, center=(0.0, -26.8, 86.0), mat=sapphire)


def build_chalice_gold() -> None:
    walnut = mat_walnut("gold_walnut", axis="X")
    gold = mat_gold()
    brass = mat_brass(rough_base=0.17)

    # Stepped walnut & brushed brass plinth
    plinth_tiered(
        "gold_plinth",
        [
            (47.0, 44.0, 9.0, walnut, 0.75),
            (39.0, 36.5, 6.5, walnut, 0.65),
            (34.5, 31.5, 4.5, brass, 0.50),
        ],
        sides=4,
        xy_ratio=0.84,
    )
    bevel_box("gold_plinth_plate", (25.0, 2.4, 5.2), (0.0, -18.5, 8.2), gold, bevel=0.45)

    # Classic athletic 24k gold two-handled chalice cup (Z = 20..113)
    profile = [
        (0.0, 19.5),
        (16.0, 19.5),
        (16.2, 22.8),
        (10.5, 26.5),
        (6.8, 34.0),
        (9.5, 40.0),
        (6.5, 46.0),
        (9.5, 52.0),
        (18.5, 60.0),
        (24.5, 74.0),
        (26.5, 92.0),
        (28.5, 106.0),
        (31.2, 111.5),
        (30.2, 113.5),
        (27.5, 113.2),
        (24.5, 102.0),
        (19.5, 78.0),
        (11.0, 64.0),
        (0.0, 61.0),
    ]
    lathe_profile("gold_chalice", profile, gold, segments=112)

    # Twin classic C-scroll athletic handles
    handle_pts = [
        (21.5, 0.0, 63.0, 2.3),
        (32.5, 0.0, 66.0, 2.5),
        (39.5, 0.0, 81.0, 2.6),
        (39.0, 0.0, 99.0, 2.3),
        (33.5, 0.0, 107.0, 2.1),
        (27.8, 0.0, 104.0, 1.9),
    ]
    curved_handle("gold_handles", handle_pts, gold, mirror_x=True)

    # Sculpted 3D golden laurel wreath wrapping the front of the cup
    laurel_wreath("gold_laurel", cx=0.0, cy=-24.0, cz=83.0, radius=13.5, bowl_r=26.2, mat=gold)


def build_goblet_silver() -> None:
    slate = mat_slate()
    silver = mat_silver()

    # Dark charcoal slate octagonal plinth + satin silver fillet
    plinth_tiered(
        "silver_plinth",
        [
            (44.0, 42.0, 8.5, slate, 0.70),
            (36.0, 33.5, 7.0, slate, 0.60),
            (29.0, 26.5, 3.0, silver, 0.45),
        ],
        sides=8,
        xy_ratio=0.84,
    )

    # Sleek satin sterling silver goblet (no side handles, distinct chalice-goblet silhouette)
    profile = [
        (0.0, 18.0),
        (15.0, 18.0),
        (15.2, 21.0),
        (9.5, 25.0),
        (5.4, 33.0),
        (8.6, 41.5),   # central stem knop
        (5.2, 49.0),
        (8.0, 56.0),
        (17.5, 65.0),
        (24.0, 79.0),
        (25.5, 95.0),
        (25.2, 107.5),
        (26.8, 111.5),  # crisp rolled goblet rim
        (25.6, 113.0),
        (23.5, 112.5),
        (22.0, 99.0),
        (18.0, 76.0),
        (10.0, 64.0),
        (0.0, 61.0),
    ]
    lathe_profile("silver_goblet", profile, silver, segments=112)

    # Raised star medallion emblem centered on the front of the goblet bowl
    # Darker antiqued pewter-silver backing disc so the raised sterling star pops crisply
    pewter = mat_brushed_metal(
        "pewter_backing",
        (0.28, 0.30, 0.34, 1.0),
        (0.38, 0.40, 0.45, 1.0),
        rough_base=0.28,
        rough_span=0.08,
        brush_scale=(0.50, 0.50, 0.08),
    )
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=48, radius1=11.0, radius2=10.2, depth=3.2)
    bmesh.ops.rotate(bm, matrix=Matrix.Rotation(math.pi / 2.0, 4, "X"), verts=bm.verts[:])
    bmesh.ops.translate(bm, vec=(0.0, -24.2, 87.0), verts=bm.verts[:])
    mesh = bpy.data.meshes.new("silver_medallion_ring")
    bm.to_mesh(mesh)
    bm.free()
    ring = link_obj("silver_medallion_ring", mesh, pewter)
    add_bevel(ring, width=0.5, segments=2)

    star_medallion("silver_star", center=(0.0, -25.4, 87.0), outer_r=8.6, inner_r=3.6, depth=2.6, mat=silver)


def build_cup_bronze() -> None:
    oak = mat_oak_dark()
    bronze = mat_bronze()

    # Aged dark oak square plinth + bronze collar
    plinth_tiered(
        "bronze_plinth",
        [
            (46.0, 43.0, 9.5, oak, 0.80),
            (38.0, 35.5, 6.0, oak, 0.65),
            (31.5, 29.0, 3.5, bronze, 0.45),
        ],
        sides=4,
        xy_ratio=0.84,
    )

    # Sturdy stout classic amphora/cup with beaded mid-band (Z = 19..109)
    profile = [
        (0.0, 18.5),
        (15.5, 18.5),
        (15.8, 22.0),
        (10.0, 26.0),
        (7.5, 34.0),
        (11.0, 41.0),
        (14.5, 48.0),
        (23.5, 58.0),
        (26.8, 72.0),
        (27.5, 76.0),   # raised mid-band bead
        (26.2, 80.0),
        (25.2, 95.0),
        (27.2, 105.5),
        (29.5, 108.5),
        (28.5, 110.2),
        (25.8, 109.8),
        (22.6, 97.0),
        (18.5, 74.0),
        (10.5, 58.0),
        (0.0, 55.0),
    ]
    lathe_profile("bronze_cup", profile, bronze, segments=112)

    # Compact grounded angular ear handles
    handle_pts = [
        (22.5, 0.0, 62.0, 2.6),
        (33.0, 0.0, 63.0, 2.7),
        (36.5, 0.0, 79.0, 2.7),
        (34.5, 0.0, 96.0, 2.4),
        (25.2, 0.0, 98.0, 2.2),
    ]
    curved_handle("bronze_handles", handle_pts, bronze, mirror_x=True)


def build_locked_trophy() -> None:
    iron = mat_iron_locked()
    brass = mat_brass(rough_base=0.22)
    matte = mat_matte_black("keyhole_void", val=0.008)

    # Dark charcoal/forged iron silhouette pedestal + trophy cup
    plinth_tiered(
        "locked_plinth",
        [
            (46.0, 43.0, 8.5, iron, 0.70),
            (38.0, 35.5, 6.5, iron, 0.60),
            (31.0, 28.5, 3.0, iron, 0.45),
        ],
        sides=8,
        xy_ratio=0.84,
    )

    profile = [
        (0.0, 17.5),
        (15.0, 17.5),
        (15.4, 21.0),
        (9.8, 25.0),
        (6.8, 33.0),
        (9.2, 41.0),
        (6.6, 48.0),
        (14.0, 58.0),
        (23.0, 72.0),
        (25.5, 92.0),
        (27.5, 106.0),
        (29.2, 110.5),
        (28.0, 112.0),
        (25.5, 111.5),
        (22.0, 96.0),
        (15.0, 72.0),
        (0.0, 60.0),
    ]
    lathe_profile("locked_cup", profile, iron, segments=96)

    handle_pts = [
        (20.0, 0.0, 63.0, 2.2),
        (31.0, 0.0, 67.0, 2.4),
        (36.0, 0.0, 83.0, 2.4),
        (34.5, 0.0, 99.0, 2.1),
        (25.8, 0.0, 101.0, 1.9),
    ]
    curved_handle("locked_handles", handle_pts, iron, mirror_x=True)

    # Prominent brushed brass keyhole padlock medallion mounted on front center
    cy_lock = -26.5
    cz_lock = 64.0
    # Curved brass shackle arch rising into the padlock body
    shackle_pts = [
        (-8.2, cy_lock, cz_lock + 10.0, 2.3),
        (-8.2, cy_lock, cz_lock + 22.5, 2.3),
        (-4.5, cy_lock, cz_lock + 28.5, 2.3),
        (4.5, cy_lock, cz_lock + 28.5, 2.3),
        (8.2, cy_lock, cz_lock + 22.5, 2.3),
        (8.2, cy_lock, cz_lock + 10.0, 2.3),
    ]
    curved_handle("locked_shackle", shackle_pts, brass, mirror_x=False)

    # Heavy beveled brass shield/disc lock body
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=48, radius1=14.5, radius2=13.5, depth=7.5)
    bmesh.ops.rotate(bm, matrix=Matrix.Rotation(math.pi / 2.0, 4, "X"), verts=bm.verts[:])
    bmesh.ops.translate(bm, vec=(0.0, cy_lock, cz_lock), verts=bm.verts[:])
    mesh = bpy.data.meshes.new("locked_padlock_body")
    bm.to_mesh(mesh)
    bm.free()
    lock_body = link_obj("locked_padlock_body", mesh, brass)
    add_bevel(lock_body, width=1.1, segments=4)

    # Inner recessed brass escutcheon ring
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=48, radius1=10.8, radius2=10.5, depth=1.6)
    bmesh.ops.rotate(bm, matrix=Matrix.Rotation(math.pi / 2.0, 4, "X"), verts=bm.verts[:])
    bmesh.ops.translate(bm, vec=(0.0, cy_lock - 3.6, cz_lock), verts=bm.verts[:])
    mesh = bpy.data.meshes.new("locked_escutcheon")
    bm.to_mesh(mesh)
    bm.free()
    esc = link_obj("locked_escutcheon", mesh, brass)
    add_bevel(esc, width=0.45, segments=2)

    # Deep dark keyhole cutout silhouette (circular bore + tapered lower ward slot)
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=32, radius1=2.8, radius2=2.8, depth=3.0)
    bmesh.ops.rotate(bm, matrix=Matrix.Rotation(math.pi / 2.0, 4, "X"), verts=bm.verts[:])
    bmesh.ops.translate(bm, vec=(0.0, cy_lock - 3.9, cz_lock + 2.4), verts=bm.verts[:])
    mesh = bpy.data.meshes.new("keyhole_top")
    bm.to_mesh(mesh)
    bm.free()
    link_obj("keyhole_top", mesh, matte)

    bm = bmesh.new()
    verts_bot = [
        bm.verts.new((-1.3, cy_lock - 5.4, cz_lock + 1.4)),
        bm.verts.new((1.3, cy_lock - 5.4, cz_lock + 1.4)),
        bm.verts.new((2.5, cy_lock - 5.4, cz_lock - 6.8)),
        bm.verts.new((-2.5, cy_lock - 5.4, cz_lock - 6.8)),
    ]
    face = bm.faces.new(verts_bot)
    extr = bmesh.ops.extrude_face_region(bm, geom=[face])
    back = [v for v in extr["geom"] if isinstance(v, bmesh.types.BMVert)]
    bmesh.ops.translate(bm, verts=back, vec=(0.0, 3.0, 0.0))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    mesh = bpy.data.meshes.new("keyhole_slot")
    bm.to_mesh(mesh)
    bm.free()
    link_obj("keyhole_slot", mesh, matte)


def render_trophy(kind: str) -> float:
    """Render one 112x144 trophy sprite and return measured pedestal bottom pixel Y."""
    cam_dist = 112.0 * (85.0 / 36.0)  # 264.444mm -> 112mm horizontal span at origin
    pitch_deg = 8.5
    rad = math.radians(pitch_deg)
    # Place target at (0, 0, 56.5) so Z=0..118 fills vertical 144px cleanly
    target = Vector((0.0, 0.0, 56.5))
    cam_loc = target + Vector((0.0, -cam_dist * math.cos(rad), cam_dist * math.sin(rad)))
    scene = setup_sprite_studio(112, 144, tuple(cam_loc), 90.0 - pitch_deg)

    if kind == "platinum":
        build_chalice_platinum()
    elif kind == "gold":
        build_chalice_gold()
    elif kind == "silver":
        build_goblet_silver()
    elif kind == "bronze":
        build_cup_bronze()
    elif kind == "locked":
        build_locked_trophy()
        # Extra crisp cool rim accent so dark iron silhouette reads cleanly against dark velvet
        add_area_light("locked_rim_boost", (-58.0, 72.0, 95.0), (0.0, 0.0, 58.0), 42.0, 100.0, 2.2, (0.72, 0.84, 1.0), spread_deg=95.0, cast_shadow=False)
        add_area_light("locked_brass_key", (28.0, -90.0, 72.0), (0.0, -28.0, 64.0), 42.0, 42.0, 0.95, (1.0, 0.90, 0.72), spread_deg=90.0, cast_shadow=False)
    else:
        raise ValueError(f"Unknown trophy kind: {kind}")

    bpy.context.view_layer.update()
    # Front-most bottom edge of pedestal sits around (0, -21, 0)
    uvz = world_to_camera_view(scene, scene.camera, Vector((0.0, -20.0, 0.0)))
    pedestal_bottom_py = (1.0 - uvz.y) * 144.0

    out = ASSETS / f"trophy_{kind}.png"
    scene.render.filepath = str(out)
    bpy.ops.render.render(write_still=True)
    return pedestal_bottom_py


# ---------------------------------------------------------------------------
# Upright Game Showcase Plaque / Easel Stand (120x120 RGBA)
# ---------------------------------------------------------------------------


def build_ribbon_trim(center_y: float, center_z: float, mat: bpy.types.Material) -> None:
    """Sculpted 3D folded brushed-brass banner ribbon with crisp swallowtail wings."""
    # Central gently arched banner plate (X in [-22, +22])
    bm = bmesh.new()
    segs_x = 20
    w_mid = 44.0
    h_mid = 9.5
    front_top, front_bot, back_top, back_bot = [], [], [], []
    for i in range(segs_x + 1):
        u = (i / segs_x) * 2.0 - 1.0  # -1..+1
        x = u * (w_mid / 2.0)
        arch_y = -1.6 * math.cos(u * math.pi * 0.5)
        front_top.append(bm.verts.new((x, center_y + arch_y - 1.6, center_z + h_mid / 2.0)))
        front_bot.append(bm.verts.new((x, center_y + arch_y - 1.6, center_z - h_mid / 2.0)))
        back_top.append(bm.verts.new((x, center_y + arch_y + 0.9, center_z + h_mid / 2.0)))
        back_bot.append(bm.verts.new((x, center_y + arch_y + 0.9, center_z - h_mid / 2.0)))
    for i in range(segs_x):
        bm.faces.new((front_bot[i], front_bot[i + 1], front_top[i + 1], front_top[i]))
        bm.faces.new((back_top[i], back_top[i + 1], back_bot[i + 1], back_bot[i]))
        bm.faces.new((front_top[i], front_top[i + 1], back_top[i + 1], back_top[i]))
        bm.faces.new((back_bot[i], back_bot[i + 1], front_bot[i + 1], front_bot[i]))
    bm.faces.new((back_bot[0], front_bot[0], front_top[0], back_top[0]))
    bm.faces.new((front_bot[-1], back_bot[-1], back_top[-1], front_top[-1]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    mesh = bpy.data.meshes.new("ribbon_center")
    bm.to_mesh(mesh)
    bm.free()
    obj = link_obj("ribbon_center", mesh, mat)
    add_bevel(obj, width=0.45, segments=2)

    # Tucked swallowtail side wings extending clearly from X=17 to X=41 slightly recessed and dropped
    for sx in (-1.0, 1.0):
        bm = bmesh.new()
        pts = [
            (sx * 16.0, center_y + 1.0, center_z + 3.0),
            (sx * 41.0, center_y + 1.8, center_z + 3.2),
            (sx * 34.5, center_y + 1.6, center_z - 1.5),
            (sx * 41.0, center_y + 1.8, center_z - 6.2),
            (sx * 16.0, center_y + 1.0, center_z - 6.0),
        ]
        f_vs = [bm.verts.new((x, y - 0.8, z)) for (x, y, z) in pts]
        b_vs = [bm.verts.new((x, y + 1.2, z)) for (x, y, z) in pts]
        bm.faces.new(f_vs if sx > 0 else list(reversed(f_vs)))
        bm.faces.new(list(reversed(b_vs)) if sx > 0 else b_vs)
        for k in range(len(pts)):
            kn = (k + 1) % len(pts)
            bm.faces.new((f_vs[k], f_vs[kn], b_vs[kn], b_vs[k]))
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
        mesh = bpy.data.meshes.new(f"ribbon_wing_{sx}")
        bm.to_mesh(mesh)
        bm.free()
        wing = link_obj(mesh.name, mesh, mat)
        add_bevel(wing, width=0.35, segments=2)


def render_plaque() -> tuple[int, int, int, int]:
    """Render 120x120 upright walnut & brass game plaque stand and return exact badge inset rect."""
    # Shift-lens horizontal camera at Z_cam = 76, centered on Z = 52 -> py = 0 at Z=112, py = 120 at Z=-8.
    # Thus every front-facing edge in XZ plane projects strictly horizontally & vertically!
    cam_dist = 120.0 * (85.0 / 36.0)  # 283.333mm -> 120mm horizontal span at Y=0
    cam_z_center = 52.0
    cam_z = 76.0
    shift_y = -(cam_z - cam_z_center) / 120.0
    scene = setup_sprite_studio(120, 120, (0.0, -cam_dist, cam_z), 90.0, shift_y=shift_y)

    walnut = mat_walnut("plaque_walnut", axis="X")
    walnut_vert = mat_walnut("plaque_walnut_v", axis="Z")
    brass = mat_brass(rough_base=0.19)
    recess_dark = mat_matte_black("plaque_recess_bg", val=0.012)

    # Frame outer dimensions: width 92mm (X in [-46, +46]), height Z in [14, 106]
    # Recessed square window opening centered at X=0, Z=66 (64x64mm at recess plane Y=4 -> exact ~64x64px!)
    # Let's build the 4 mitered-look walnut frame bars around X in [-33, +33], Z in [33, 99] (66x66 opening with inner brass bezel leaving ~64x64 dark window!)
    win_half = 33.5
    win_zc = 66.0
    # Backing body behind window (recessed matte dark panel at Y = +4.5)
    bevel_box("plaque_recess_backing", (74.0, 4.0, 74.0), (0.0, 6.0, win_zc), recess_dark, bevel=0.0)
    # Walnut upright board back
    bevel_box("plaque_back_slab", (90.0, 10.0, 92.0), (0.0, 11.5, 60.0), walnut_vert, bevel=1.0)

    # 4 frame surround rails (left, right, top, bottom)
    frame_y = 1.5
    frame_depth = 10.0
    bevel_box("frame_top", (92.0, frame_depth, 10.5), (0.0, frame_y, win_zc + win_half + 5.25), walnut, bevel=0.9)
    bevel_box("frame_bot", (92.0, frame_depth, 21.0), (0.0, frame_y, win_zc - win_half - 10.5), walnut, bevel=0.9)
    bevel_box("frame_left", (12.5, frame_depth, win_half * 2.0 + 2.0), (-win_half - 6.25, frame_y, win_zc), walnut_vert, bevel=0.9)
    bevel_box("frame_right", (12.5, frame_depth, win_half * 2.0 + 2.0), (win_half + 6.25, frame_y, win_zc), walnut_vert, bevel=0.9)

    # Inner stepped brushed-brass bezel rim framing the square inset (opening width = 64.0mm!)
    in_half = 32.0
    bz_w = 2.6
    bz_y = 0.0
    bevel_box("bezel_top", (in_half * 2 + bz_w * 2, 7.0, bz_w), (0.0, bz_y, win_zc + in_half + bz_w / 2.0), brass, bevel=0.45)
    bevel_box("bezel_bot", (in_half * 2 + bz_w * 2, 7.0, bz_w), (0.0, bz_y, win_zc - in_half - bz_w / 2.0), brass, bevel=0.45)
    bevel_box("bezel_left", (bz_w, 7.0, in_half * 2), (-in_half - bz_w / 2.0, bz_y, win_zc), brass, bevel=0.45)
    bevel_box("bezel_right", (bz_w, 7.0, in_half * 2), (in_half + bz_w / 2.0, bz_y, win_zc), brass, bevel=0.45)

    # Decorative corner brass L-brackets on outer walnut frame
    for sx in (-1.0, 1.0):
        bevel_box(f"top_corner_cap_{sx}", (8.5, 11.2, 4.0), (sx * 42.0, frame_y - 0.5, win_zc + win_half + 9.0), brass, bevel=0.4)

    # 3D sculpted brass ribbon banner trim below the bezel (Z ~ 22)
    build_ribbon_trim(center_y=-4.0, center_z=23.5, mat=brass)

    # Upright stepped walnut & brass easel base foot resting solidly on Z = 0
    bevel_box("easel_foot_walnut", (98.0, 26.0, 9.0), (0.0, 6.0, 4.5), walnut, bevel=1.0)
    bevel_box("easel_foot_brass", (95.0, 22.0, 3.2), (0.0, 4.5, 10.6), brass, bevel=0.45)
    # Rear easel kick-stand strut (subtle 3D depth)
    bevel_box("easel_strut", (22.0, 16.0, 42.0), (0.0, 22.0, 22.0), walnut_vert, bevel=0.8)

    # Measure exact pixel rectangle of the inner dark square window (at bezel front opening Y = 0)
    bpy.context.view_layer.update()
    tl = world_to_camera_view(scene, scene.camera, Vector((-in_half, 0.0, win_zc + in_half)))
    br = world_to_camera_view(scene, scene.camera, Vector((in_half, 0.0, win_zc - in_half)))
    rx0 = round(tl.x * 120.0)
    ry0 = round((1.0 - tl.y) * 120.0)
    rx1 = round(br.x * 120.0)
    ry1 = round((1.0 - br.y) * 120.0)

    out = ASSETS / "trophy_plaque.png"
    scene.render.filepath = str(out)
    bpy.ops.render.render(write_still=True)
    return rx0, ry0, rx1 - rx0, ry1 - ry0


# ---------------------------------------------------------------------------
# Entrypoint & Post-Render Verification
# ---------------------------------------------------------------------------


def composite_contact_shadow(
    path: Path,
    cx: float,
    cy: float,
    rx_core: float,
    ry_core: float,
    rx_halo: float,
    ry_halo: float,
) -> None:
    """Composite a smooth warm-umbra ambient contact shadow underneath a rendered sprite."""
    img = bpy.data.images.load(str(path))
    w, h = img.size
    buf = np.empty(w * h * 4, dtype=np.float32)
    img.pixels.foreach_get(buf)
    # Blender stores bottom-up; flip to top-down pixel coordinates
    fg = buf.reshape(h, w, 4)[::-1].copy()
    bpy.data.images.remove(img)

    yy, xx = np.mgrid[0:h, 0:w]
    d_core = np.hypot((xx - cx) / rx_core, (yy - cy) / ry_core)
    d_halo = np.hypot((xx - cx) / rx_halo, (yy - cy) / ry_halo)

    a_core = 0.62 * np.clip(1.0 - d_core, 0.0, 1.0) ** 1.7
    a_halo = 0.30 * np.clip(1.0 - d_halo, 0.0, 1.0) ** 2.2
    sa = np.clip(np.maximum(a_core, a_halo), 0.0, 0.72)

    # Warm dark umber contact shadow color (sRGB)
    sr, sg, sb = 0.035, 0.018, 0.010

    fa = fg[..., 3]
    out_a = fa + sa * (1.0 - fa)
    safe_a = np.maximum(out_a, 1e-6)
    fg[..., 0] = np.where(out_a > 1e-6, (fg[..., 0] * fa + sr * sa * (1.0 - fa)) / safe_a, 0.0)
    fg[..., 1] = np.where(out_a > 1e-6, (fg[..., 1] * fa + sg * sa * (1.0 - fa)) / safe_a, 0.0)
    fg[..., 2] = np.where(out_a > 1e-6, (fg[..., 2] * fa + sb * sa * (1.0 - fa)) / safe_a, 0.0)
    fg[..., 3] = out_a

    # Save with Standard/None color management so already-AgX sRGB pixels are written verbatim
    scene = bpy.context.scene
    prev_vt = scene.view_settings.view_transform
    prev_lk = scene.view_settings.look
    scene.view_settings.view_transform = "Standard"
    scene.view_settings.look = "None"
    out_img = bpy.data.images.new(path.stem + "_shadowed", w, h, alpha=True)
    out_img.pixels.foreach_set(fg[::-1].reshape(-1))
    out_img.filepath_raw = str(path)
    out_img.file_format = "PNG"
    out_img.save()
    bpy.data.images.remove(out_img)
    scene.view_settings.view_transform = prev_vt
    scene.view_settings.look = prev_lk


def inspect_png(path: Path) -> tuple[int, int, int, int, int]:
    """Read rendered PNG via Blender image buffer and return (w, h, bytes, min_alpha, max_alpha)."""
    img = bpy.data.images.load(str(path))
    w, h = img.size
    buf = np.empty(w * h * 4, dtype=np.float32)
    img.pixels.foreach_get(buf)
    rgba = buf.reshape(h, w, 4)[::-1]
    alpha = (np.clip(rgba[..., 3], 0.0, 1.0) * 255.0 + 0.5).astype(np.uint8)
    bpy.data.images.remove(img)
    return w, h, path.stat().st_size, int(alpha.min()), int(alpha.max())


def main() -> None:
    wanted = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    all_targets = ["room", "platinum", "gold", "silver", "bronze", "locked", "plaque"]
    targets = [t for t in all_targets if not wanted or t in wanted]

    for t in targets:
        if t == "room":
            scene, _cam, m = build_trophy_room()
            out = ASSETS / "trophy_room.png"
            scene.render.filepath = str(out)
            bpy.ops.render.render(write_still=True)
            w, h, sz, amin, amax = inspect_png(out)
            print(f"ASSET {out.name} {w}x{h} {sz}B alpha=[{amin},{amax}]")
            print(
                f"ROOM_METRICS "
                f"x_front=({m['x_front_min']:.1f},{m['x_front_max']:.1f}) "
                f"x_back=({m['x_back_min']:.1f},{m['x_back_max']:.1f}) "
                f"top_shelf_y=(back={m['y_top_back']:.1f},rest={m['y_top_rest']:.1f},front={m['y_top_front']:.1f}) "
                f"bot_shelf_y=(back={m['y_bot_back']:.1f},rest={m['y_bot_rest']:.1f},front={m['y_bot_front']:.1f}) "
                f"header_y=({m['y_hdr_top']:.1f},{m['y_hdr_bot']:.1f}) "
                f"plaque_y=({m['y_plq_top']:.1f},{m['y_plq_bot']:.1f})"
            )
        elif t == "plaque":
            rx, ry, rw, rh = render_plaque()
            out = ASSETS / "trophy_plaque.png"
            composite_contact_shadow(out, cx=60.0, cy=111.0, rx_core=47.0, ry_core=3.2, rx_halo=53.0, ry_halo=5.8)
            w, h, sz, amin, amax = inspect_png(out)
            print(
                f"ASSET {out.name} {w}x{h} {sz}B alpha=[{amin},{amax}] "
                f"badge_rect_px=({rx},{ry},{rw},{rh}) "
                f"badge_rect_norm=({rx / w:.4f},{ry / h:.4f},{rw / w:.4f},{rh / h:.4f})"
            )
        else:
            ped_y = render_trophy(t)
            out = ASSETS / f"trophy_{t}.png"
            composite_contact_shadow(out, cx=56.0, cy=ped_y - 1.5, rx_core=26.5, ry_core=4.2, rx_halo=35.0, ry_halo=7.2)
            w, h, sz, amin, amax = inspect_png(out)
            print(f"ASSET {out.name} {w}x{h} {sz}B alpha=[{amin},{amax}] pedestal_bottom_y={ped_y:.1f}")


if __name__ == "__main__":
    main()
