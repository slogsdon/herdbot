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

    write(
        &root.join("alpha/bot.toml"),
        "name = \"alpha\"\ntitle = \"Alpha\"\n",
    );
    let turns: Vec<String> = (0..4)
        .map(|i| {
            format!(
                r#"{{"at":{i},"prompt":"q{i}","reply":"a{i}","messages":[{{"source":"user"}}]}}"#
            )
        })
        .collect();
    write(
        &root.join("alpha/turns.jsonl"),
        &format!("{}\nnot json\n", turns.join("\n")),
    );
    write(
        &root.join(".queue/alpha.jsonl"),
        "{\"text\":\"queued one\",\"source\":\"user\",\"at\":50}\n{\"text\":\"queued two\",\"source\":\"user\",\"at\":51}\n",
    );
    write(
        &root.join(".state/inflight/alpha.json"),
        &format!(
            r#"{{"prompt":"working on it","at":{},"messages":[{{"source":"user"}}]}}"#,
            home::now()
        ),
    );

    // Newest page: persisted items are numbered; pending ones follow with no number.
    let all = history::history("alpha", None, 50).unwrap();
    assert_eq!((all.v, all.total, all.items.len()), (1, 8, 11));
    assert_eq!(all.warnings, ["skipped 1 unparsable line(s)"]);
    assert_eq!(
        (
            all.items[0].n,
            all.items[0].kind,
            all.items[0].text.as_str()
        ),
        (Some(0), "user", "q0")
    );
    assert_eq!(
        (
            all.items[1].n,
            all.items[1].kind,
            all.items[1].text.as_str()
        ),
        (Some(1), "bot", "a0")
    );
    let pending: Vec<_> = all.items[8..]
        .iter()
        .map(|i| (i.n, i.state, i.text.as_str()))
        .collect();
    assert_eq!(
        pending,
        [
            (None, Some("inflight"), "working on it"),
            (None, Some("queued"), "queued one"),
            (None, Some("queued"), "queued two")
        ]
    );

    // Paging from the end: the last 3 persisted items, then the 3 before them.
    let page = history::history("alpha", None, 3).unwrap();
    let numbers: Vec<_> = page.items.iter().map(|i| i.n).collect();
    assert_eq!(numbers, [Some(5), Some(6), Some(7), None, None, None]);
    let older = history::history("alpha", Some(5), 3).unwrap();
    let numbers: Vec<_> = older.items.iter().map(|i| i.n).collect();
    assert_eq!(numbers, [Some(2), Some(3), Some(4)]);
    // An older page has no pending items; a page past the start is just short.
    assert!(
        history::history("alpha", Some(2), 10)
            .unwrap()
            .items
            .iter()
            .all(|i| i.n.is_some())
    );
    assert!(
        history::history("alpha", Some(0), 10)
            .unwrap()
            .items
            .is_empty()
    );

    // A stale marker is a crash's leftover, not a bot still working.
    write(
        &root.join(".state/inflight/alpha.json"),
        r#"{"prompt":"old","at":1}"#,
    );
    assert!(
        history::history("alpha", None, 50)
            .unwrap()
            .items
            .iter()
            .all(|i| i.state != Some("inflight"))
    );
    home::write_inflight("alpha", "fresh").unwrap();
    assert_eq!(home::read_inflight("alpha").unwrap().prompt, "fresh");
    home::clear_inflight("alpha");
    assert!(home::read_inflight("alpha").is_none());

    // A prompt-less turn (typed in the pane) is a bot item alone.
    write(&root.join("beta/bot.toml"), "name = \"beta\"\n");
    write(
        &root.join("beta/turns.jsonl"),
        "{\"at\":1,\"prompt\":null,\"reply\":\"typed\"}\n",
    );
    let beta = history::history("beta", None, 50).unwrap();
    assert_eq!(beta.items.len(), 1);
    assert_eq!(beta.items[0].kind, "bot");

    // Typed prompts retain their queue identity, source, and original text in history. Legacy
    // prompts remain unknown even when their text resembles a generated bot message.
    write(&root.join("provenance/bot.toml"), "name = \"provenance\"\n");
    write(
        &root.join("provenance/turns.jsonl"),
        concat!(
            "{\"at\":1,\"id\":\"turn-bot\",\"prompt\":\"joined prompt\",\"reply\":\"handled\",\"messages\":[{\"id\":\"queue-bot\",\"source\":\"bot_message\",\"text\":\"[message from beta] review this\"}]}\n",
            "{\"at\":2,\"id\":\"turn-user\",\"prompt\":\"typed by you\",\"reply\":\"answer\",\"messages\":[{\"id\":\"queue-user\",\"source\":\"user\",\"text\":\"typed by you\"}]}\n",
            "{\"at\":3,\"id\":\"turn-legacy-wrapper\",\"prompt\":\"[message from alpha] old wrapper\",\"reply\":\"reply\"}\n",
            "{\"at\":4,\"id\":\"turn-legacy-plain\",\"prompt\":\"ordinary legacy prompt\",\"reply\":\"reply\"}\n",
        ),
    );
    let provenance = history::history("provenance", None, 50).unwrap();
    let prompts: Vec<_> = provenance.items.iter().filter(|item| item.kind != "bot").collect();
    assert_eq!(prompts[0].id.as_deref(), Some("queue-bot"));
    assert_eq!(prompts[0].source, Some("bot_message"));
    assert_eq!(prompts[0].kind, "notice");
    assert_eq!(prompts[0].text, "[message from beta] review this");
    assert_eq!(prompts[1].id.as_deref(), Some("queue-user"));
    assert_eq!(prompts[1].source, Some("user"));
    assert_eq!(prompts[1].kind, "user");
    assert_eq!(prompts[2].source, Some("unknown"));
    assert_eq!(prompts[2].kind, "unknown");
    assert_eq!(prompts[2].text, "[message from alpha] old wrapper");
    assert_eq!(prompts[3].source, Some("unknown"));
    assert_eq!(prompts[3].kind, "unknown");

    home::enqueue_with_id("provenance", "queue-pending", "bot_message", "queued bot text").unwrap();
    let queued = history::queued_items("provenance");
    assert_eq!(queued[0].id.as_deref(), Some("queue-pending"));
    assert_eq!(queued[0].source, Some("bot_message"));
    assert_eq!(queued[0].kind, "notice");
    home::write_inflight_with_messages(
        "provenance",
        "joined inflight text",
        vec![home::QueueReference {
            id: Some("inflight-bot".into()),
            source: "bot_message".into(),
            text: Some("inflight bot text".into()),
        }],
    )
    .unwrap();
    let inflight = history::pending_items("provenance");
    assert_eq!(inflight[0].id.as_deref(), Some("inflight-bot"));
    assert_eq!(inflight[0].source, Some("bot_message"));
    assert_eq!(inflight[0].text, "inflight bot text");

    // Run lifecycle entries merge with ordinary turns, retain detail, and deduplicate stable IDs.
    let run_dir = root.join(".runs");
    write(
        &run_dir.join("r1.json"),
        r#"{"id":"r1","bot":"alpha","parent_agent":"alpha","agent":"sub1","pane":"w:p1","class":"code","route":"pi:model","description":"first","status":"done","result":"Full report\nwith detail","started_at":1,"summary":"Review summary","events":[{"id":"shared-progress","at":2,"kind":"progress","summary":"working","detail":"full progress"},{"id":"r1-done","at":5,"kind":"completed","summary":"Review summary"},{"id":"r1-blocked","at":7,"kind":"blocked","summary":"approval needed"},{"id":"r1-failed","at":8,"kind":"failed","summary":"failed"},{"id":"r1-stopped","at":9,"kind":"stopped","summary":"stopped"},{"id":"r1-expired","at":10,"kind":"expired","summary":"expired"}]}"#,
    );
    write(
        &run_dir.join("r2.json"),
        r#"{"id":"r2","bot":"alpha","parent_agent":"alpha","agent":"sub2","pane":"w:p2","class":"code","route":"pi:model","description":"second","status":"done","result":"Fallback summary\ncomplete report","started_at":1,"events":[{"id":"shared-progress","at":3,"kind":"progress","summary":"duplicate"},{"id":"r2-done","at":6,"kind":"completed","summary":""}]}"#,
    );
    use std::io::Write as _;
    let mut alpha_turns = fs::OpenOptions::new()
        .append(true)
        .open(root.join("alpha/turns.jsonl"))
        .unwrap();
    writeln!(
        alpha_turns,
        r#"{{"id":"turn-r1","at":6,"prompt":"completed task notice","messages":[{{"id":"r1-done","source":"lifecycle","text":"completed task notice"}}]}}"#
    )
    .unwrap();
    let merged = history::history("alpha", None, 50).unwrap();
    assert_eq!(merged.v, 1);
    assert_eq!(
        merged.items.iter().map(|item| item.at).collect::<Vec<_>>(),
        {
            let mut timestamps = merged.items.iter().map(|item| item.at).collect::<Vec<_>>();
            timestamps.sort();
            timestamps
        }
    );
    let lifecycle: Vec<_> = merged
        .items
        .iter()
        .filter(|item| item.source == Some("lifecycle"))
        .collect();
    assert_eq!(lifecycle.len(), 7);
    assert_eq!(
        merged.items.iter().filter(|item| item.id.as_deref() == Some("r1-done")).count(),
        1,
        "the queued wake-up and persisted lifecycle event share one history row"
    );
    assert_eq!(lifecycle[0].id.as_deref(), Some("shared-progress"));
    assert_eq!(lifecycle[0].kind, "notice");
    assert_eq!(lifecycle[0].text, "working");
    assert_eq!(lifecycle[0].detail.as_deref(), Some("full progress"));
    assert_eq!(lifecycle[1].text, "Review summary");
    assert_eq!(
        lifecycle[1].detail.as_deref(),
        Some("Full report\nwith detail")
    );
    assert_eq!(lifecycle[2].text, "Fallback summary");
    assert_eq!(
        lifecycle[2].detail.as_deref(),
        Some("Fallback summary\ncomplete report")
    );
    assert_eq!(
        lifecycle[3..]
            .iter()
            .map(|item| item.state.unwrap())
            .collect::<Vec<_>>(),
        ["blocked", "failed", "stopped", "expired"]
    );
    let legacy_item = serde_json::to_value(&merged.items[0]).unwrap();
    assert!(legacy_item.get("id").is_none());
    assert_eq!(legacy_item["source"], "user");
    assert!(legacy_item.get("detail").is_none());

    // Rooms: you, bots (with their slug), notices; no pending items.
    write(
        &root.join("rooms/crew/room.toml"),
        "name = \"Crew\"\nmembers = [\"alpha\", \"beta\"]\n",
    );
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
    let kinds: Vec<_> = room
        .items
        .iter()
        .map(|i| (i.kind, i.speaker.as_deref()))
        .collect();
    assert_eq!(
        kinds,
        [("user", None), ("bot", Some("alpha")), ("notice", None)]
    );
    assert_eq!(room.warnings, ["skipped 1 unparsable line(s)"]);

    // Unknown targets are errors, not empty conversations.
    assert!(history::history("nobody", None, 50).is_err());
    assert!(history::history("room:nowhere", None, 50).is_err());

    let _ = fs::remove_dir_all(&root);
}
