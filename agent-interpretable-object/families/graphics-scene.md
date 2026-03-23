# Graphics Scene Family

This note defines the reusable family that should eventually unify:

- `presentation`
- `svg`
- `figma`
- `blender`

## Core Idea

All of these can be treated as scene-like objects:

- they contain positioned parts
- those parts have geometry
- those parts have style or material
- those parts may be grouped
- those parts may be layered or ordered
- those parts may be viewed through projections

## Shared Semantic Units

- `scene`
- `node`
- `group`
- `frame`
- `geometry`
- `transform`
- `style`
- `material`
- `camera`
- `viewport`
- `timeline`

## Shared Relations

- `contains`
- `groups`
- `positions`
- `styles`
- `transforms`
- `layers_before`
- `references`
- `animates`

## Shared Views

- scene view
- outline view
- layer view
- geometry view
- material/style view
- timeline view

## Practical Extension Path

### SVG

Treat SVG as a mostly 2D graphics-scene realization:

- scene -> svg root
- node -> element
- geometry -> path/rect/circle/etc.
- style -> fill/stroke/text style
- transform -> transform attribute

### Figma

Treat Figma as a design-scene realization:

- scene -> file/page
- frame -> frame
- node -> layer/component/instance
- style -> paint/effect/text style
- relation -> auto-layout/component binding/prototype edge

### Blender

Treat Blender as a 3D scene realization:

- scene -> scene
- node -> object/bone/collection
- geometry -> mesh/curve/volume
- material -> shader graph
- transform -> object transform
- camera -> camera
- timeline -> animation data

## Why This Matters

If the family is defined well, new kinds do not need a whole new philosophy.
They only need:

1. a mapping from file/domain terms into the shared family
2. kind-specific invariants
3. kind-specific capabilities and loss boundaries
