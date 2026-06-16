extends Node

var server: FlyRulerServer

func _ready() -> void:
    server = FlyRulerServer.new()

    var ok := server.start_server("127.0.0.1:8080")
    if not ok:
        push_error("Failed to start FlyRulerServer")
        return

    print("FlyRuler server listening on: ", server.local_addr())

func _process(_delta: float) -> void:
    if server == null or not server.is_running():
        return

    var ids: PackedStringArray = server.get_aircraft_ids()
    for id in ids:
        var state := server.get_latest_state(id)
        if state.is_empty():
            continue
        # Example fields
        var pos: Dictionary = state.get("position", {})
        var ts: float = float(state.get("timestamp_secs", 0.0))
        # print("id=", id, " ts=", ts, " x=", pos.get("x", 0.0))

func save_current_session() -> bool:
    return server.save_session("user://sessions/latest")

func load_saved_session() -> bool:
    return server.load_session("user://sessions/latest")

func _exit_tree() -> void:
    if server != null:
        server.stop_server()
