<!-- SPDX-License-Identifier: AGPL-3.0-only -->
# Demo slate pictures

Two stills for the demo station, Kettle FM (ADR-0200): the station's own, and the demo night
show's. The slate shows the show on air's picture, else the station's. Each is one icon, chosen
by the operator from the estate's icon picker on 2026-11-03, drawn at 288 px in the design
system's ink-2 (`#a9b1d6`) and centred on its dark surface (`#1a1b26`), with no words: the
slate's band carries the station, the show and the track. The SVG is the source; the PNG is
its 1280 × 720 render (`magick`, librsvg), the size the slate fits a picture to.

| File | Picture | Icon | Set | Licence | Source |
|---|---|---|---|---|---|
| `kettle-fm.svg`, `kettle-fm.png` | the station's | `mdi:kettle-outline` | Material Design Icons (Pictogrammers) | Apache-2.0 (`LICENSES/Apache-2.0.txt`) | `https://api.iconify.design/mdi/kettle-outline.svg`, from `https://github.com/Templarian/MaterialDesign` |
| `night-show.svg`, `night-show.png` | the demo night show's | `line-md:moon` | Material Line Icons (Vjacheslav Trushkin) | MIT (`LICENSES/MIT.txt`) | `https://api.iconify.design/line-md/moon.svg`, from `https://github.com/cyberalien/line-md` |

The kettle is the icon's path unmodified. `line-md:moon` is an animated icon; the still is its
final frame: the stroke's dash and the stars' fade-in are removed, nothing else is changed.

Set them with the show editor's **Slate picture** field (the Schedule tab, the Show level) and
desk B's **Slate picture** control, or with the API:

```sh
curl -X PUT -H "Authorization: Bearer $KEY" -H 'Content-Type: image/png' --data-binary @kettle-fm.png "$STATION/api/v1/video/still"
curl -X PUT -H "Authorization: Bearer $KEY" -H 'Content-Type: image/png' --data-binary @night-show.png "$STATION/api/v1/shows/$SHOW/still"
```
