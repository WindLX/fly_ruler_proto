# Linux addon layout

The installer produces the following Godot 4 addon layout:

```text
res://addons/fly_ruler_proto/
  fly_ruler_proto_godot.gdextension
  libfly_ruler_proto_godot.so
  fly_ruler_runtime_example.gd
  README.md
  manifest.json
  web/
    index.html
    assets/
```

The `.gdextension` intentionally advertises Linux x86_64 only. Web files are served directly by the embedded management server and must remain ordinary filesystem assets in exported builds.
