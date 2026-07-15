# Patchwright app icon

`PatchwrightIcon-source.svg` is the original, editable vector source for the
Patchwright identity. It was created for this project and contains no Apple,
GitHub, Codex, or other third-party marks. The centered drafting compass and
steel nib close an amber seam between two offset patch tiles.

`PatchwrightIcon-source.png` is the committed 1024×1024 sRGB raster master used
by `script/generate_app_icon.sh`. The PNG can be reproduced from the SVG with
librsvg 2.62.3:

```bash
rsvg-convert --keep-aspect-ratio --width 1024 --height 1024 \
  --output Assets/PatchwrightIcon-source.png Assets/PatchwrightIcon-source.svg
```

The deployable `Packaging/Patchwright.icns` is generated from the committed PNG
master. Do not substitute generated artwork, a third-party logo, or an icon
that contains readable text.
