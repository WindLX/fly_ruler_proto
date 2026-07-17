extends Node

var runtime: Node

func _ready() -> void:
	runtime = ClassDB.instantiate(&"FlyRulerRuntime") as Node
	add_child(runtime)
	var config := ClassDB.instantiate(&"FlyRulerRuntimeConfig") as Object
	runtime.connect(&"snapshot_published", _on_snapshot_published)
	runtime.call(&"start", config)

func _on_snapshot_published(snapshot: Object) -> void:
	var aircraft: Array = snapshot.get(&"aircraft") as Array
	print("FlyRuler frame aircraft: %d" % aircraft.size())
