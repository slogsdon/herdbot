use std::fs;
use std::io::Write;
use std::os::unix::net::UnixListener;
use std::path::Path;

use herdbot_core::{home, snapshot};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn append(path: &Path, text: &str) {
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "{text}").unwrap();
}

/// One test: it owns HERDBOT_HOME for this whole process.
#[test]
fn snapshot_and_watch_over_a_fixture_home() {
    // Short path: unix socket paths top out near 104 bytes on macOS.
    let root = std::env::temp_dir().join(format!("hbc-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    // SAFETY: this file has a single test, so nothing else touches the environment.
    unsafe { std::env::set_var("HERDBOT_HOME", &root) };

    write(&root.join("alpha/bot.toml"), "name = \"alpha\"\ntitle = \"Alpha\"\n");
    write(&root.join("beta/bot.toml"), "name = \"beta\"\ntitle = \"Beta\"\n");
    write(&root.join("broken/bot.toml"), "not = [toml");
    write(
        &root.join(".state/alpha.json"),
        r#"{"workspace_id":"w1","root_pane":"p1","agent":"alpha-main"}"#,
    );
    let run = |id: &str, status: &str| {
        format!(
            r#"{{"id":"{id}","bot":"alpha","parent_agent":"alpha-main","agent":"alpha-{id}","pane":"p2","class":"code","route":"claude:sonnet","description":"do it","status":"{status}","started_at":1}}"#
        )
    };
    write(&root.join("rooms/crew/room.toml"), "name = \"Crew\"\nmembers = [\"alpha\", \"beta\"]\nmode = \"round_robin\"\n");
    write(&root.join("rooms/quiet/room.toml"), "name = \"Quiet\"\nmembers = [\"alpha\", \"beta\"]\n");
    write(&root.join(".state/rooms/quiet.json"), r#"{"paused":true}"#);
    write(&root.join("rooms/torn/room.toml"), "mode = \"chaos\"");
    write(
        &root.join("alpha/turns.jsonl"),
        "{\"at\":5,\"prompt\":\"hi\",\"reply\":\"\\n  Hello there\\nmore\"}\nnot json\n{\"at\":7,\"prompt\":null,\"reply\":\"typed in the pane\"}\n",
    );
    write(&root.join(".queue/alpha.jsonl"), "{\"text\":\"first\",\"at\":30}\n{\"text\":\"second\",\"at\":31}\n");
    write(&root.join(".state/inflight/beta.json"), &format!(r#"{{"prompt":"p","at":{}}}"#, home::now()));
    write(&root.join("rooms/crew/transcript.jsonl"), "{\"at\":9,\"speaker\":\"you\",\"text\":\"hello crew\"}\n");
    write(&root.join(".runs/r1.json"), &run("r1", "running"));
    write(&root.join(".runs/r2.json"), &run("r2", "done"));
    write(&root.join(".state/cooldown/claude.json"), r#"{"until":4102444800,"reason":"limit"}"#);
    write(&root.join(".state/cooldown/codex.json"), r#"{"until":1,"reason":"old"}"#);
    let blocked = r#"{"at":10,"agent":"alpha-main","kind":"blocked","reason":"approval: Bash","harness":"claude"}"#;
    let turn_end = r#"{"at":11,"agent":"beta-main","kind":"turn_end","text":"ok","harness":"claude"}"#;
    write(&root.join(".state/events/alpha-main.jsonl"), &format!("{blocked}\n"));
    write(
        &root.join(".state/events/beta-main.jsonl"),
        &format!("{}\nnot json\n{turn_end}\n", blocked.replace("alpha-main", "beta-main")),
    );

    let snap = snapshot::snapshot();
    assert_eq!(snap.v, 1);
    assert_eq!(snap.daemon, "down");
    let bots: Vec<_> = snap.bots.iter().map(|b| (b.slug.as_str(), b.up)).collect();
    assert_eq!(bots, vec![("alpha", true), ("beta", false)]);
    // alpha: user+bot from the first turn, bot only from the second; one bad line skipped.
    assert_eq!((snap.bots[0].items, snap.bots[0].last_at, snap.bots[0].preview.as_str()), (3, Some(7), "typed in the pane"));
    assert!(!snap.bots[0].working);
    assert_eq!((snap.bots[0].queued, snap.bots[1].queued), (2, 0));
    assert_eq!((snap.bots[1].items, snap.bots[1].last_at, snap.bots[1].preview.as_str()), (0, None, ""));
    assert!(snap.bots[1].working);
    let rooms: Vec<_> = snap.rooms.iter().map(|r| (r.slug.as_str(), r.name.as_str(), r.paused)).collect();
    assert_eq!(rooms, vec![("crew", "Crew", false), ("quiet", "Quiet", true)]);
    assert_eq!((snap.rooms[0].items, snap.rooms[0].last_at, snap.rooms[0].preview.as_str()), (1, Some(9), "hello crew"));
    assert!(snap.warnings.iter().any(|w| w.starts_with("room torn:")));
    assert_eq!(snap.runs.len(), 1);
    assert_eq!(snap.runs[0].id, "r1");
    assert_eq!(snap.cooldowns.len(), 1);
    assert_eq!(snap.cooldowns[0].harness, "claude");
    assert_eq!(snap.blocked.len(), 1);
    assert_eq!(snap.blocked[0].agent, "alpha-main");
    assert_eq!(snap.blocked[0].reason.as_deref(), Some("approval: Bash"));
    assert!(snap.warnings.iter().any(|w| w.starts_with("broken:")));
    assert!(snap.warnings.iter().any(|w| w.contains("beta-main.jsonl")));

    // The JSON shape the Swift agent depends on.
    let json = serde_json::to_value(&snap).unwrap();
    let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["at", "blocked", "bots", "cooldowns", "daemon", "rooms", "runs", "v", "warnings"]);

    assert_eq!(
        json["bots"][0],
        serde_json::json!({"slug":"alpha","title":"Alpha","up":true,"items":3,"last_at":7,"preview":"typed in the pane","working":false,"queued":2})
    );
    assert_eq!(json["rooms"][0], serde_json::json!({"slug":"crew","name":"Crew","mode":"round_robin","paused":false,"items":1,"last_at":9,"preview":"hello crew"}));

    // Watcher: first poll is the snapshot only, then quiet, then changes.
    let mut watcher = snapshot::Watcher::default();
    let first = watcher.poll();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["type"], "snapshot");
    assert_eq!(first[0]["v"], 1);
    assert!(watcher.poll().is_empty());

    append(
        &root.join(".state/events/alpha-main.jsonl"),
        r#"{"at":20,"agent":"alpha-main","kind":"turn_end","text":"done","harness":"claude"}"#,
    );
    let next = watcher.poll();
    let types: Vec<_> = next.iter().map(|l| l["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["snapshot", "event"]);
    assert_eq!(next[0]["snapshot"]["blocked"].as_array().unwrap().len(), 0);
    assert_eq!(next[1]["event"]["kind"], "turn_end");

    // A live daemon accepts connections on its lock socket.
    let _listener = UnixListener::bind(home::daemon_lock()).unwrap();
    assert_eq!(snapshot::snapshot().daemon, "up");

    let _ = fs::remove_dir_all(&root);
}
