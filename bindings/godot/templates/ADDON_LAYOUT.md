# Recommended Addon Layout

Copy or generate this structure inside your Godot project:

```text
res://addons/fly_ruler_proto/
  fly_ruler_proto_godot.gdextension
  libfly_ruler_proto_godot.so          # Linux
  fly_ruler_proto_godot.dll            # Windows
  libfly_ruler_proto_godot.dylib       # macOS
  FlyRulerDemo.gd                      # Optional demo script
```

Notes:
- Keep file names consistent with `fly_ruler_proto_godot.gdextension`.
- `.gdextension` should point to the real binary names on your target platform.
- You can keep both debug/release binaries and switch by build/export setup.
