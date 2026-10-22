//! Rooms: group conversations between bots and Shane. Plain files under `rooms/<slug>/`; each
//! daemon tick routes new transcript lines to members' queues. Design:
//! docs/superpowers/specs/2026-10-20-rooms-design.md.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;

use serde::{Deserialize, Serialize};

use crate::home::{self, Result};
pub use herdbot_core::rooms::{
    Mode, Room, Round, State, list_rooms, load_room, load_state, room_dir,
};
use herdbot_core::rooms::state_path;

pub fn save_room(slug: &str, room: &Room) -> Result<()> {
    fs::create_dir_all(room_dir(slug))?;
    fs::write(room_dir(slug).join("room.toml"), toml::to_string_pretty(room)?)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub at: i64,
    /// "you", a bot slug, or "herdbot".
    pub speaker: String,
    pub text: String,
}

pub fn validate(room: &Room, bots: &[String]) -> std::result::Result<(), String> {
    if room.members.len() < 2 {
        return Err("a room needs at least two members".into());
    }
    if let Some(m) = room.members.iter().find(|m| !bots.contains(m)) {
        return Err(format!("no bot {m:?}; known: {}", bots.join(", ")));
    }
    if room.mode == Mode::Moderated
        && !room.moderator.as_ref().is_some_and(|m| room.members.contains(m))
    {
        return Err("a moderated room needs --moderator, one of its members".into());
    }
    Ok(())
}

/// One JSON line. serde_json escapes newlines in `text`, so an entry is always one line.
fn entry_line(speaker: &str, text: &str, at: i64) -> String {
    let entry = Entry { at, speaker: speaker.to_owned(), text: text.to_owned() };
    format!("{}\n", serde_json::to_string(&entry).unwrap_or_default())
}

/// Append one entry. A single write on an append-mode file, so concurrent writers (room send,
/// room_post, the daemon's notices) never interleave; line numbers are the sequence.
pub fn append(slug: &str, speaker: &str, text: &str) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(room_dir(slug).join("transcript.jsonl"))?;
    file.write_all(entry_line(speaker, text, home::now()).as_bytes())?;
    Ok(())
}

/// Every line, `None` for a malformed one, so index `i` is always sequence `i + 1`.
pub fn read_entries(slug: &str) -> Vec<Option<Entry>> {
    fs::read_to_string(room_dir(slug).join("transcript.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).ok())
        .collect()
}

pub fn save_state(slug: &str, state: &State) -> Result<()> {
    fs::create_dir_all(state_path(slug).parent().unwrap())?;
    home::write_json(&state_path(slug), state)
}

/// transcript.md: what the `room up` pane tails and the GUI will read.
pub fn render_md(entries: &[Option<Entry>]) -> String {
    entries
        .iter()
        .flatten()
        .map(|e| {
            let time = chrono::DateTime::from_timestamp(e.at, 0)
                .map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string())
                .unwrap_or_default();
            format!("**{}** · {time}\n{}\n\n", e.speaker, e.text)
        })
        .collect()
}

/// A member to prompt with transcript lines `after + 1 ..= through`.
#[derive(Debug, Clone, PartialEq)]
pub struct Delivery {
    pub member: String,
    pub after: usize,
    pub through: usize,
}

/// `needle` occurs in `hay` with no word character (alphanumeric, `-`, `_`) on either side.
fn has_word(hay: &str, needle: &str) -> bool {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_');
    !needle.is_empty()
        && hay.match_indices(needle).any(|(i, _)| {
            !word(hay[..i].chars().next_back()) && !word(hay[i + needle.len()..].chars().next())
        })
}

/// Members named in `text` by `@slug`, or by display name as a whole word, case-insensitive.
/// Deleted members (absent from `names`) are still found by `@slug`.
pub fn mentioned(text: &str, members: &[String], names: &BTreeMap<String, String>) -> Vec<String> {
    let text = text.to_lowercase();
    members
        .iter()
        .filter(|m| {
            has_word(&text, &format!("@{}", m.to_lowercase()))
                || names.get(*m).is_some_and(|n| has_word(&text, &n.to_lowercase()))
        })
        .cloned()
        .collect()
}

fn missing_notice(member: &str) -> String {
    format!("[herdbot: {member} is not a bot any more; skipped]")
}

/// round_robin: call the first existing member at or after `from`, or end the round.
fn start_at(
    room: &Room,
    names: &BTreeMap<String, String>,
    st: &mut State,
    from: usize,
    now: i64,
    notices: &mut Vec<String>,
) -> Vec<String> {
    for (i, m) in room.members.iter().enumerate().skip(from) {
        if names.contains_key(m) {
            st.round = Some(Round { next: i, called_at: now });
            return vec![m.clone()];
        }
        if st.missing_notified.insert(m.clone()) {
            notices.push(missing_notice(m));
        }
    }
    st.round = None;
    Vec::new()
}

/// Who speaks next. Pure: the daemon reads the files, calls this, then writes notices,
/// queues prompts and saves the returned state.
pub fn route(
    room: &Room,
    names: &BTreeMap<String, String>,
    mut st: State,
    new: &[Option<Entry>],
    first_seq: usize,
    now: i64,
) -> (Vec<Delivery>, Vec<String>, State) {
    let mut notices = Vec::new();
    let mut calls: BTreeSet<String> = BTreeSet::new();
    for entry in new.iter().flatten() {
        let speaker = entry.speaker.as_str();
        match speaker {
            "herdbot" => continue,
            "you" => {
                st.bot_turns = 0;
                st.paused = false;
            }
            bot => {
                st.last_post.insert(bot.to_owned(), entry.at);
                if st.paused {
                    continue;
                }
                st.bot_turns += 1;
                if st.bot_turns >= room.max_turns {
                    st.paused = true;
                    notices.push(format!(
                        "[room paused: {} bot turns since your last message. Post to resume.]",
                        st.bot_turns
                    ));
                    continue;
                }
            }
        }
        let named = || mentioned(&entry.text, &room.members, names);
        let next: Vec<String> = match room.mode {
            Mode::Channel => named(),
            Mode::Moderated => {
                let moderator = room.moderator.clone().unwrap_or_default();
                if speaker == moderator { named() } else { vec![moderator] }
            }
            Mode::RoundRobin => {
                let turn = st.round.as_ref().map(|r| r.next);
                if speaker == "you" {
                    start_at(room, names, &mut st, 0, now, &mut notices)
                } else if let Some(i) = turn
                    && room.members.get(i).is_some_and(|m| m == speaker)
                {
                    start_at(room, names, &mut st, i + 1, now, &mut notices)
                } else {
                    Vec::new()
                }
            }
        };
        calls.extend(next.into_iter().filter(|m| m != speaker));
    }
    if !st.paused
        && let Some(r) = st.round.clone()
        && now - r.called_at >= room.turn_timeout_secs
    {
        notices.push(format!("[herdbot: {} did not answer]", room.members[r.next]));
        calls.extend(start_at(room, names, &mut st, r.next + 1, now, &mut notices));
    }
    for m in &calls {
        if !names.contains_key(m) && st.missing_notified.insert(m.clone()) {
            notices.push(missing_notice(m));
        }
    }
    calls.retain(|m| names.contains_key(m));
    if !new.is_empty() {
        st.cursor = first_seq + new.len() - 1;
    }
    let through = st.cursor;
    let due: BTreeSet<String> = calls.into_iter().chain(std::mem::take(&mut st.pending)).collect();
    let mut deliveries = Vec::new();
    for m in due {
        let cooling = st
            .last_post
            .get(&m)
            .is_some_and(|&t| now - t < room.member_cooldown_secs);
        if cooling {
            st.pending.insert(m);
            continue;
        }
        let after = st.last_seen.get(&m).copied().unwrap_or(0);
        if after < through {
            st.last_seen.insert(m.clone(), through);
            deliveries.push(Delivery { member: m, after, through });
        }
    }
    (deliveries, notices, st)
}

/// The queued prompt for one delivery: every line in range not written by the member.
pub fn delivery_prompt(slug: &str, room: &Room, entries: &[Option<Entry>], d: &Delivery) -> Option<String> {
    let lines: Vec<String> = entries
        .get(d.after..d.through.min(entries.len()))?
        .iter()
        .flatten()
        .filter(|e| e.speaker != d.member)
        .map(|e| format!("{}: {}", e.speaker, e.text))
        .collect();
    (!lines.is_empty()).then(|| {
        format!(
            "[room {slug} \"{}\"]\n{}\n(Reply with room_post room=\"{slug}\". Name members with @slug to bring them in. Members: {}.)",
            room.name,
            lines.join("\n"),
            room.members.join(", ")
        )
    })
}

pub fn check_member(room: &Room, bot: &str, in_run: bool) -> std::result::Result<(), String> {
    if in_run {
        return Err("rooms are for a bot's main agent, not subagent runs".into());
    }
    if !room.members.iter().any(|m| m == bot) {
        return Err(format!("{bot} is not a member of {:?}", room.name));
    }
    Ok(())
}

/// One daemon pass over every room.
pub fn tick(now: i64) -> Result<()> {
    let names: BTreeMap<String, String> = home::list_bots()
        .into_iter()
        .filter_map(|s| home::load_bot(&s).ok().map(|b| (s, b.name)))
        .collect();
    for slug in list_rooms() {
        let room = match load_room(&slug) {
            Ok(room) => room,
            Err(e) => {
                eprintln!("room {slug}: {e}");
                continue;
            }
        };
        let entries = read_entries(&slug);
        let state = load_state(&slug);
        let start = state.cursor.min(entries.len());
        let new = &entries[start..];
        if new.iter().any(Option::is_none) {
            eprintln!("room {slug}: skipped malformed transcript line(s)");
        }
        let (deliveries, notices, state) = route(&room, &names, state, new, start + 1, now);
        for d in &deliveries {
            if let Some(text) = delivery_prompt(&slug, &room, &entries, d) {
                home::enqueue(&crate::run::main_agent(&d.member), &text)?;
                println!("room {slug}: called {}", d.member);
            }
        }
        for n in &notices {
            append(&slug, "herdbot", n)?;
        }
        // ponytail: state is saved after enqueueing, so a crash in between re-delivers once.
        save_state(&slug, &state)?;
        if !new.is_empty() || !notices.is_empty() {
            fs::write(room_dir(&slug).join("transcript.md"), render_md(&read_entries(&slug)))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use herdbot_core::rooms::parse_room;

    use super::*;

    fn room(mode: Mode) -> Room {
        Room {
            name: "Crew".into(),
            members: vec!["ann".into(), "bob".into(), "cy".into()],
            mode,
            moderator: (mode == Mode::Moderated).then(|| "ann".into()),
            ..Default::default()
        }
    }

    fn names() -> BTreeMap<String, String> {
        [("ann", "Ann"), ("bob", "Bob"), ("cy", "Cy")]
            .map(|(s, n)| (s.to_owned(), n.to_owned()))
            .into()
    }

    fn e(speaker: &str, text: &str, at: i64) -> Option<Entry> {
        Some(Entry { at, speaker: speaker.into(), text: text.into() })
    }

    fn called(d: &[Delivery]) -> Vec<&str> {
        d.iter().map(|d| d.member.as_str()).collect()
    }

    #[test]
    fn mentions_are_whole_words() {
        let m = room(Mode::Channel).members;
        assert_eq!(mentioned("@bob, and Cy?", &m, &names()), ["bob", "cy"]);
        assert_eq!(mentioned("ANN please", &m, &names()), ["ann"]);
        assert!(mentioned("Bobby and email@bobcat", &m, &names()).is_empty());
        // A deleted member is still named by @slug, so routing can report it.
        let mut gone = names();
        gone.remove("cy");
        assert_eq!(mentioned("@cy", &m, &gone), ["cy"]);
    }

    #[test]
    fn channel_calls_named_members_only() {
        let r = room(Mode::Channel);
        let (d, n, st) = route(&r, &names(), State::default(), &[e("you", "hi @bob and Cy", 100)], 1, 100);
        assert_eq!(called(&d), ["bob", "cy"]);
        assert!(n.is_empty());
        assert_eq!((d[0].after, d[0].through, st.cursor), (0, 1, 1));
        // Naming nobody calls nobody.
        let (d, _, st) = route(&r, &names(), st, &[e("you", "anyone?", 101)], 2, 101);
        assert!(d.is_empty());
        assert_eq!(st.cursor, 2);
        // A speaker never calls itself.
        let (d, _, _) = route(&r, &names(), st, &[e("bob", "@bob @ann thoughts?", 102)], 3, 102);
        assert_eq!(called(&d), ["ann"]);
        assert_eq!((d[0].after, d[0].through), (0, 3));
    }

    #[test]
    fn pauses_after_max_turns_until_you_post() {
        let mut r = room(Mode::Channel);
        r.max_turns = 2;
        r.member_cooldown_secs = 0;
        let new = [e("ann", "@bob", 1), e("bob", "@ann", 2), e("ann", "@bob", 3)];
        let (d, n, st) = route(&r, &names(), State::default(), &new, 1, 3);
        // Turn 1 calls bob; turn 2 pauses the room and calls nobody; turn 3 is paused.
        assert_eq!(called(&d), ["bob"]);
        assert_eq!(n, ["[room paused: 2 bot turns since your last message. Post to resume.]"]);
        assert!(st.paused);
        // The herdbot notice neither counts nor resumes; your message does.
        let (d, _, st) = route(&r, &names(), st, &[e("herdbot", &n[0], 4), e("you", "go on @ann", 5)], 4, 5);
        assert!(!st.paused);
        assert_eq!(st.bot_turns, 0);
        assert_eq!(called(&d), ["ann"]);
    }

    #[test]
    fn cooldown_defers_and_coalesces() {
        let r = room(Mode::Channel); // 30 s cooldown
        let new = [e("bob", "done", 100), e("ann", "@bob again", 105)];
        let (d, _, st) = route(&r, &names(), State::default(), &new, 1, 105);
        assert!(d.is_empty());
        assert!(st.pending.contains("bob"));
        let (d, _, st) = route(&r, &names(), st, &[e("cy", "@bob too", 110)], 3, 110);
        assert!(d.is_empty());
        // Cooldown over: one delivery covering everything bob has not seen.
        let (d, _, st) = route(&r, &names(), st, &[], 4, 131);
        assert_eq!(called(&d), ["bob"]);
        assert_eq!((d[0].after, d[0].through), (0, 3));
        assert!(st.pending.is_empty());
    }

    #[test]
    fn round_robin_goes_in_order_once() {
        let mut r = room(Mode::RoundRobin);
        r.member_cooldown_secs = 0;
        let (d, _, st) = route(&r, &names(), State::default(), &[e("you", "topic @cy", 1)], 1, 1);
        assert_eq!(called(&d), ["ann"]); // names are ignored
        let (d, _, st) = route(&r, &names(), st, &[e("cy", "jumping in", 2)], 2, 2);
        assert!(d.is_empty()); // not cy's turn
        let (d, _, st) = route(&r, &names(), st, &[e("ann", "my take", 3)], 3, 3);
        assert_eq!(called(&d), ["bob"]);
        let (d, _, st) = route(&r, &names(), st, &[e("bob", "mine", 4)], 4, 4);
        assert_eq!(called(&d), ["cy"]);
        let (d, _, st) = route(&r, &names(), st, &[e("cy", "last", 5)], 5, 5);
        assert!(d.is_empty());
        assert_eq!(st.round, None);
    }

    #[test]
    fn round_robin_skips_a_silent_member_and_restarts_on_you() {
        let r = room(Mode::RoundRobin); // 300 s timeout
        let (_, _, st) = route(&r, &names(), State::default(), &[e("you", "go", 0)], 1, 0);
        let (d, n, st) = route(&r, &names(), st, &[], 2, 299);
        assert!(d.is_empty() && n.is_empty());
        let (d, n, st) = route(&r, &names(), st, &[], 2, 300);
        assert_eq!(n, ["[herdbot: ann did not answer]"]);
        assert_eq!(called(&d), ["bob"]);
        let new = [e("herdbot", "[herdbot: ann did not answer]", 300), e("you", "again", 301)];
        let (d, _, st) = route(&r, &names(), st, &new, 2, 301);
        assert_eq!(called(&d), ["ann"]);
        assert_eq!(st.round.as_ref().unwrap().next, 0);
    }

    #[test]
    fn moderated_relays_through_the_moderator() {
        let mut r = room(Mode::Moderated); // moderator ann
        r.member_cooldown_secs = 0;
        let (d, _, st) = route(&r, &names(), State::default(), &[e("you", "@bob what do you think", 1)], 1, 1);
        assert_eq!(called(&d), ["ann"]);
        let (d, _, st) = route(&r, &names(), st, &[e("ann", "@bob your turn", 2)], 2, 2);
        assert_eq!(called(&d), ["bob"]);
        let (d, _, _) = route(&r, &names(), st, &[e("bob", "@cy here", 3)], 3, 3);
        assert_eq!(called(&d), ["ann"]);
    }

    #[test]
    fn missing_members_are_reported_once_and_skipped() {
        let mut r = room(Mode::Channel);
        r.member_cooldown_secs = 0;
        let mut gone = names();
        gone.remove("cy");
        let (d, n, st) = route(&r, &gone, State::default(), &[e("you", "@cy @bob", 1)], 1, 1);
        assert_eq!(called(&d), ["bob"]);
        assert_eq!(n, ["[herdbot: cy is not a bot any more; skipped]"]);
        let (_, n, _) = route(&r, &gone, st, &[e("you", "@cy?", 2)], 2, 2);
        assert!(n.is_empty());
        // round_robin skips them in the order.
        let mut rr = room(Mode::RoundRobin);
        rr.member_cooldown_secs = 0;
        let mut no_ann = names();
        no_ann.remove("ann");
        let (d, n, _) = route(&rr, &no_ann, State::default(), &[e("you", "go", 1)], 1, 1);
        assert_eq!(called(&d), ["bob"]);
        assert_eq!(n, ["[herdbot: ann is not a bot any more; skipped]"]);
    }

    #[test]
    fn delivery_prompt_lists_unseen_lines_from_others() {
        let r = room(Mode::Channel);
        let entries = [e("you", "hi @bob", 1), None, e("bob", "earlier", 2), e("cy", "@bob +1", 3)];
        let d = Delivery { member: "bob".into(), after: 0, through: 4 };
        assert_eq!(
            delivery_prompt("crew", &r, &entries, &d).unwrap(),
            "[room crew \"Crew\"]\nyou: hi @bob\ncy: @bob +1\n(Reply with room_post room=\"crew\". Name members with @slug to bring them in. Members: ann, bob, cy.)"
        );
        // Only its own posts in range: nothing to send.
        let own = Delivery { member: "bob".into(), after: 2, through: 3 };
        assert_eq!(delivery_prompt("crew", &r, &entries, &own), None);
    }

    #[test]
    fn only_main_agents_of_members_post() {
        let r = room(Mode::Channel);
        assert_eq!(check_member(&r, "ann", false), Ok(()));
        assert!(check_member(&r, "zed", false).unwrap_err().contains("not a member"));
        assert!(check_member(&r, "ann", true).unwrap_err().contains("main agent"));
    }

    #[test]
    fn room_toml_defaults_and_validation() {
        let room = parse_room("name = \"Crew\"\nmembers = [\"ann\", \"bob\"]\n").unwrap();
        assert_eq!(room.mode, Mode::Channel);
        assert_eq!((room.max_turns, room.member_cooldown_secs, room.turn_timeout_secs), (6, 30, 300));
        let rr = parse_room("name = \"X\"\nmembers = [\"a\", \"b\"]\nmode = \"round_robin\"\n").unwrap();
        assert_eq!(rr.mode, Mode::RoundRobin);
        assert!(parse_room("mode = \"chaos\"").is_err());

        let bots = vec!["ann".to_owned(), "bob".to_owned()];
        assert_eq!(validate(&room, &bots), Ok(()));
        let solo = Room { members: vec!["ann".into()], ..room.clone() };
        assert!(validate(&solo, &bots).unwrap_err().contains("two"));
        let stranger = Room { members: vec!["ann".into(), "zed".into()], ..room.clone() };
        assert!(validate(&stranger, &bots).unwrap_err().contains("zed"));
        let unmoderated = Room { mode: Mode::Moderated, ..room.clone() };
        assert!(validate(&unmoderated, &bots).unwrap_err().contains("moderator"));
        let moderated = Room { mode: Mode::Moderated, moderator: Some("bob".into()), ..room };
        assert_eq!(validate(&moderated, &bots), Ok(()));
    }

    #[test]
    fn transcript_lines_round_trip_and_render() {
        let line = entry_line("ann", "hello\nthere", 0);
        assert!(line.ends_with('\n') && line.matches('\n').count() == 1);
        let entry: Entry = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!((entry.speaker.as_str(), entry.text.as_str()), ("ann", "hello\nthere"));
        let md = render_md(&[Some(entry), None]);
        assert!(md.starts_with("**ann** · "));
        assert!(md.contains("\nhello\nthere\n"));
    }
}
