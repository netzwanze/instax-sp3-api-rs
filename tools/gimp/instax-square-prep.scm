#!/usr/bin/env gimp-script-fu-interpreter-3.0

(define (script-fu-instax-square image drawables)
  (let* ((drawable (vector-ref drawables 0)))
    (gimp-image-undo-group-start image)

    ; 1. Auf 800x800 Pixel skalieren (NoHalo = 3)
    (gimp-context-set-interpolation 3)
    (gimp-image-scale image 800 800)

    ; 2. Mitteltöne leicht aufhellen (Kurve auf Wert-Kanal)
    (gimp-drawable-curves-spline drawable 0 #(0.0 0.0 0.45 0.52 1.0 1.0))

    ; 3. Sättigung leicht anheben (+12)
    (gimp-drawable-hue-saturation drawable 0 0.0 0.0 12.0 0.0)

    ; 4. Leicht nachschärfen via GEGL-Filter
    (gimp-drawable-merge-new-filter
      drawable
      "gegl:unsharp-mask"
      "Instax sharpen"
      0
      1.0
      "std-dev" 1.2
      "scale" 0.4
      "threshold" 0.0)

    (gimp-image-undo-group-end image)
    (gimp-displays-flush)
  )
)

(script-fu-register-filter
  "script-fu-instax-square"
  "Prepare Instax Square (800x800)"
  "Scales the image to 800x800px, brightens midtones, raises saturation and sharpens the image"
  "netzwanze"
  "MIT"
  "2026"
  "RGB*"
  SF-ONE-DRAWABLE
)

(script-fu-menu-register "script-fu-instax-square" "<Image>/Filters/Instax")
