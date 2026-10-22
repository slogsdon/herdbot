use std::fs;
use std::path::Path;

use herdbot_core::{history, home};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// One test: it owns HERDBOT_HOME for this whole process.
#[test]
fn history_pages_and_appends_what_is_pending() {
    let root = std::env::temp_dir().join(format!("hbh-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    // SAFETY: this file has a single test, so nothing else touches the environment.
    unsafe { std::env::set_var("HERDBOT_HOME", &root) };

    write(&root.join("alpha/bot.toml"), "name = \"alpha\"\ntitle = \"Alpha\"\n");
    let turns: Vec<String> = (0..4)
        .map(|i| format!(r#"{{"at":{i},"prompt":"q{i}","reply":"a{i}"}}"#))
        .collect();
    write(&root.join("alpha/turns.jsonl"), &format!("{}\nnot json\n", turns.join("\n")));
    write(
        &root.join(".queue/alpha.jsonl"),
        "{\"text\":\"queued one\",\"at\":50}\n{\"text\":\"queued two\",\"at\":51}\n",
    );
    write(&root.join(".state/inflight/alpha.json"), &format!(r#"{{"prompt":"working on it","at":{}}}"#, home::now()));

    // Newest page: persisted items are numbered; pending ones follow with no number.
    let all = history::history("alpha", None, 50).unwrap();
    assert_eq!((all.v, all.total, all.items.len()), (1, 8, 11));
    assert_eq!(all.warnings, ["skipped 1 unparsable line(s)"]);
    assert_eq!((all.items[0].n, all.items[0].kind, all.items[0].text.as_str()), (Some(0), "user", "q0"));
    assert_eq!((all.items[1].n, all.items[1].kind, all.items[1].text.as_str()), (Some(1), "bot", "a0"));
    let pending: Vec<_> = all.items[8..].iter().map(|i| (i.n, i.state, i.text.as_str())).collect();
    assert_eq!(
        pending,
        [(None, Some("inflight"), "working on it"), (None, Some("queued"), "queued one"), (None, Some("queued"), "queued two")]
    );

    // Paging from the end: the last 3 persisted items, then the 3 before them.
    let page = history::history("alpha", None, 3).unwrap();
    let numbers: Vec<_> = page.items.iter().map(|i| i.n).collect();
    assert_eq!(numbers, [Some(5), Some(6), Some(7), None, None, None]);
    let older = history::history("alpha", Some(5), 3).unwrap();
    let numbers: Vec<_> = older.items.iter().map(|i| i.n).collect();
    assert_eq!(numbers, [Some(2), Some(3), Some(4)]);
    // An older page has no pending items; a page past the start is just short.
    assert!(history::history("alpha", Some(2), 10).unwrap().items.iter().all(|i| i.n.is_some()));
    assert!(history::history("alpha", Some(0), 10).unwrap().items.is_empty());

    // A stale marker is a crash's leftover, not a bot still working.
    write(&root.join(".state/inflight/alpha.json"), r#"{"prompt":"old","at":1}"#);
    assert!(history::history("alpha", None, 50).unwrap().items.iter().all(|i| i.state != Some("inflight")));
    home::write_inflight("alpha", "fresh").unwrap();
    assert_eq!(home::read_inflight("alpha").unwrap().prompt, "fresh");
    home::clear_inflight("alpha");
    assert!(home::read_inflight("alpha").is_none());

    // A prompt-less turn (typed in the pane) is a bot item alone.
    write(&root.join("beta/bot.toml"), "name = \"beta\"\n");
    write(&root.join("beta/turns.jsonl"), "{\"at\":1,\"prompt\":null,\"reply\":\"typed\"}\n");
    let beta = history::history("beta", None, 50).unwrap();
    assert_eq!(beta.items.len(), 1);
    assert_eq!(beta.items[0].kind, "bot");

    // Rooms: you, bots (with their slug), notices; no pending items.
    write(&root.join("rooms/crew/room.toml"), "name = \"Crew\"\nmembers = [\"alpha\", \"beta\"]\n");
    write(
        &root.join("rooms/crew/transcript.jsonl"),
        concat!(
            "{\"at\":1,\"speaker\":\"you\",\"text\":\"hi all\"}\n",
            "{\"at\":2,\"speaker\":\"alpha\",\"text\":\"hello\"}\n",
            "oops\n",
            "{\"at\":3,\"speaker\":\"herdbot\",\"text\":\"beta is down\"}\n",
        ),
    );
    let room = history::history("room:crew", None, 50).unwrap();
    assert_eq!(room.total, 3);
    let kinds: Vec<_> = room.items.iter().map(|i| (i.kind, i.speaker.as_deref())).collect();
    assert_eq!(kinds, [("user", None), ("bot", Some("alpha")), ("notice", None)]);
    assert_eq!(room.warnings, ["skipped 1 unparsable line(s)"]);

    // Unknown targets are errors, not empty conversations.
    assert!(history::history("nobody", None, 50).is_err());
    assert!(history::history("room:nowhere", None, 50).is_err());

    let _ = fs::remove_dir_all(&root);
}
