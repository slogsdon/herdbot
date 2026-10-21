//! OptChat-style memory for a bot: one endless log of its prompts and replies, a binary tree
//! of ≤512-byte summaries, and a fixed-size view folded from them. Design:
//! docs/superpowers/specs/2026-10-20-optchat-memory-design.md. The daemon is the only writer.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use herdr_client::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::home::{self, Result};

pub const NODE: usize = 512;
pub const TRIES: usize = 5;
pub const RETRY_SECS: i64 = 10;
/// A compactor starts after the log has been quiet this long (or at once for a recycle).
pub const QUIET_SECS: i64 = 60;
pub const DEFAULT_VIEW: usize = 64_000;
const PLACEHOLDER: &str = "(not summarized yet: zoom it)";

/// A realistic summary line of exactly NODE bytes, shown to the compactor for scale.
pub const SCALE: &str = "user: keep the router table in routing.toml, never in code and route explore to cheap models first; talk: moved explore candidates to pi deepseek-v4-flash then claude haiku, kept plan on opus; user: asked why codex lost its first prompt; talk: traced it to the trust modal on first run in a new cwd, fix is --trust for cursor and a resend for codex; user: approved shipping the resend; work: subagent k3x9a1 ran the tests, 13 passed, committed 9b4b9a8; talk: still open: does claude quota text need a date parser";

pub const COMPACT: &str = include_str!("../assets/optchat-compact.md");
pub const VIEW_DOC: &str = include_str!("../assets/optchat-view-doc.md");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub i: usize,
    /// "user" or "talk".
    pub kind: String,
    pub text: String,
    pub size: usize,
    pub date: i64,
    /// The turns.jsonl line it came from: the import cursor lives in the log itself, so a
    /// deleted state file or a crash never imports a turn twice.
    #[serde(default)]
    pub t: usize,
}

impl Message {
    pub fn new(i: usize, kind: &str, text: &str, date: i64) -> Self {
        Message { i, kind: kind.into(), text: text.into(), size: kind.len() + 2 + text.len(), date, t: 0 }
    }

    pub fn line(&self) -> String {
        format!("{}: {}", self.kind, self.text)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub l: u32,
    pub i: usize,
    pub text: String,
    pub size: usize,
}

/// (level, index): node (l, i) covers messages [i·2^l, (i+1)·2^l).
pub type Key = (u32, usize);

/// First message a node covers, and how many it covers.
pub fn span((l, i): Key) -> (usize, usize) {
    (i << l, 1 << l)
}

pub fn flat(text: &str) -> String {
    text.replace('\n', " ")
}

#[derive(Debug, Default, Clone)]
pub struct Mem {
    pub root: Vec<Message>,
    pub tree: HashMap<Key, String>,
    /// Parts tiling the log, oldest first.
    pub view: Vec<Key>,
    pub budget: usize,
}

impl Mem {
    pub fn new(budget: usize) -> Self {
        Mem { budget, ..Default::default() }
    }

    pub fn built(&self, k: Key) -> bool {
        self.tree.contains_key(&k)
    }

    /// Its sources exist: the message for level 0, both children above.
    fn ready(&self, (l, i): Key) -> bool {
        if l == 0 {
            i < self.root.len()
        } else {
            self.built((l - 1, 2 * i)) && self.built((l - 1, 2 * i + 1))
        }
    }

    /// The node's text when no model is needed: its source already fits in NODE bytes.
    fn free_text(&self, (l, i): Key) -> Option<String> {
        let text = if l == 0 {
            self.root.get(i)?.line()
        } else {
            format!("{}\n{}", self.tree.get(&(l - 1, 2 * i))?, self.tree.get(&(l - 1, 2 * i + 1))?)
        };
        (text.len() <= NODE).then_some(text)
    }

    /// Build free nodes from `k` up through its ancestors while they are ready and free.
    fn build_free_from(&mut self, mut k: Key) {
        loop {
            if !self.built(k) {
                match self.free_text(k) {
                    Some(text) => {
                        self.tree.insert(k, text);
                    }
                    None => return,
                }
            }
            let parent = (k.0 + 1, k.1 / 2);
            if !self.ready(parent) {
                return;
            }
            k = parent;
        }
    }

    pub fn append(&mut self, m: Message) {
        let i = self.root.len();
        self.root.push(Message { i, ..m });
        self.view.push((0, i));
        self.build_free_from((0, i));
        self.fit();
    }

    /// A node a compactor built.
    pub fn insert(&mut self, k: Key, text: String) {
        self.tree.insert(k, text);
        let parent = (k.0 + 1, k.1 / 2);
        if self.ready(parent) {
            self.build_free_from(parent);
        }
        self.fit();
    }

    fn part_len(&self, k: Key) -> usize {
        self.tree.get(&k).map_or(PLACEHOLDER.len(), String::len)
    }

    pub fn size(&self) -> usize {
        self.view.iter().map(|&k| self.part_len(k)).sum()
    }

    /// While over budget, merge the most due adjacent pair whose parent is built (OptMem's age
    /// rule: due = age / 2^(l+2)). Merged parts are never split.
    fn fit(&mut self) {
        let total = self.root.len() as f64;
        let mut size = self.size();
        while size > self.budget {
            let best = (0..self.view.len().saturating_sub(1))
                .filter(|&w| {
                    let (a, b) = (self.view[w], self.view[w + 1]);
                    a.0 == b.0 && a.1.is_multiple_of(2) && b.1 == a.1 + 1 && self.built((a.0 + 1, a.1 / 2))
                })
                .map(|w| {
                    let a = self.view[w];
                    (w, (total - span(a).0 as f64) / f64::from(1u32 << (a.0 + 2)))
                })
                // Most due; on a tie, the older pair.
                .max_by(|x, y| x.1.total_cmp(&y.1).then(y.0.cmp(&x.0)));
            let Some((w, _)) = best else { break };
            let (a, b) = (self.view[w], self.view[w + 1]);
            let parent = (a.0 + 1, a.1 / 2);
            size = size - self.part_len(a) - self.part_len(b) + self.part_len(parent);
            self.view.splice(w..w + 2, [parent]);
        }
    }

    /// First message whose view part is unbuilt (the spec's `first`).
    pub fn first_unbuilt(&self) -> usize {
        self.view.iter().find(|&&k| !self.built(k)).map_or(self.root.len(), |&k| span(k).0)
    }

    pub fn settled(&self) -> bool {
        self.view.iter().all(|&k| self.built(k))
    }

    /// The next node for a compactor, in OptChat's order (spec §4.1 rule 3): unbuilt, its
    /// sources built, everything in the view before its end summarized, not waiting on a retry.
    pub fn due(&self, failed: &HashMap<Key, i64>, now: i64) -> Option<Key> {
        let total = self.root.len();
        let first = self.first_unbuilt();
        let mut l = 0u32;
        while (1usize << l) <= total {
            let mut i = 0;
            while (i + 1) << l <= total {
                let k = (l, i);
                let end = if l == 0 { i } else { (i + 1) << l };
                if !self.built(k)
                    && self.ready(k)
                    && end <= first
                    && failed.get(&k).is_none_or(|&t| now >= t)
                {
                    return Some(k);
                }
                i += 1;
            }
            l += 1;
        }
        None
    }

    pub fn render(&self) -> String {
        let mut out = String::from("<chat>\n");
        for &k in &self.view {
            let (id, n) = span(k);
            let text = self.tree.get(&k).map_or(PLACEHOLDER.to_owned(), |t| flat(t));
            out.push_str(&format!("{id}+{n}|{text}\n"));
        }
        out.push_str("</chat>");
        out
    }

    /// Where a node's compactor context ends: before message i for level 0 (the message itself
    /// comes whole in the step), through its last message for a merge.
    pub fn job_end((l, i): Key) -> usize {
        if l == 0 { i } else { (i + 1) << l }
    }

    /// The compactor's context: bare view lines (no ids, newlines flattened) of the parts that
    /// end at or before `end`. Unbuilt parts are left out; rule 3 means there are none before a
    /// due node anyway.
    pub fn context_lines(&self, end: usize) -> Vec<String> {
        self.view
            .iter()
            .filter(|&&k| span(k).0 + span(k).1 <= end)
            .filter_map(|k| self.tree.get(k).map(|t| flat(t)))
            .collect()
    }

    /// One job for a compactor. The first job of a run carries the whole context; later ones
    /// carry only the context lines that changed since the previous job.
    pub fn job_prompt(&self, (l, i): Key, context: &[String], first: bool) -> String {
        let context = if first {
            format!("<chat>\n{}\n</chat>\n\n", context.join("\n"))
        } else if context.is_empty() {
            String::new()
        } else {
            format!("New lines at the end of <chat> since your last job:\n{}\n\n", context.join("\n"))
        };
        let step = if l == 0 {
            format!("Compress this message into one line, in at most {NODE} bytes:\n{}", self.root[i].line())
        } else {
            format!(
                "Merge these two lines into one, in at most {NODE} bytes:\n{}\n{}",
                flat(&self.tree[&(l - 1, 2 * i)]),
                flat(&self.tree[&(l - 1, 2 * i + 1)])
            )
        };
        format!(
            "{context}For scale, this line is exactly {NODE} bytes:\n{SCALE}\n\n{step}\n\nAnswer by calling write_node with the line as text."
        )
    }

    /// The spec's zoom: n = 1 gives message id whole; larger n the node's two children.
    pub fn zoom(&self, id: usize, n: usize) -> String {
        if n == 0 || !n.is_power_of_two() || !id.is_multiple_of(n) || id + n > self.root.len() {
            return format!("No line {id}+{n}.");
        }
        if n == 1 {
            return format!("{id}+0|{}", self.root[id].line());
        }
        let (l, i) = (n.trailing_zeros(), id / n);
        [(l - 1, 2 * i), (l - 1, 2 * i + 1)]
            .iter()
            .map(|&k| {
                let (s, c) = span(k);
                format!("{s}+{c}|{}", self.tree.get(&k).map_or(PLACEHOLDER.to_owned(), |t| flat(t)))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn date(&self, id: usize) -> String {
        self.root
            .get(id)
            .and_then(|m| chrono::DateTime::from_timestamp(m.date, 0))
            .map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| format!("No message {id}."))
    }
}

/// The view as the daemon rebuilds it at load: the stored tree, then every message in order
/// (spec §5.2 "At load").
pub fn fold(messages: Vec<Message>, nodes: Vec<Node>, budget: usize) -> Mem {
    let mut mem = Mem::new(budget);
    mem.tree = nodes.into_iter().map(|n| ((n.l, n.i), n.text)).collect();
    for m in messages {
        mem.append(m);
    }
    mem
}

/// Context lines past the longest prefix the two lists share.
pub fn delta(prev: &[String], next: &[String]) -> Vec<String> {
    let common = prev.iter().zip(next).take_while(|(a, b)| a == b).count();
    next[common..].to_vec()
}

#[derive(Debug)]
pub enum Verdict {
    Accept(String),
    /// Ask again with this cut-at-limit feedback (spec §4.3).
    Retry(String),
    /// An empty line: the node failed.
    Fail,
}

pub fn check(tries: &[String]) -> Verdict {
    let last = tries.last().map(|t| t.trim().to_owned()).unwrap_or_default();
    if last.is_empty() {
        return Verdict::Fail;
    }
    if last.len() <= NODE {
        return Verdict::Accept(last);
    }
    if tries.len() >= TRIES {
        let shortest = tries.iter().map(|t| t.trim()).min_by_key(|t| t.len()).unwrap_or_default();
        return Verdict::Accept(shortest.to_owned());
    }
    let mut cut = NODE;
    while !last.is_char_boundary(cut) {
        cut -= 1;
    }
    Verdict::Retry(format!(
        "That line is {} bytes; the limit is {NODE}. It must end where it is cut here:\n{}| ← LIMIT",
        last.len(),
        &last[..cut]
    ))
}

/// One turns.jsonl line (number `t`) → its messages: `user` for the delivered prompt (absent
/// when typed in the pane), then `talk` for the reply.
pub fn turn_messages(line: &serde_json::Value, next: usize, t: usize) -> Vec<Message> {
    let at = line["at"].as_i64().unwrap_or_default();
    let mut out = Vec::new();
    if let Some(prompt) = line["prompt"].as_str() {
        out.push(Message { t, ..Message::new(next, "user", prompt, at) });
    }
    if let Some(reply) = line["reply"].as_str() {
        out.push(Message { t, ..Message::new(next + out.len(), "talk", reply, at) });
    }
    out
}

/// Messages from `turns.jsonl` not yet in the log. The import position is the last logged
/// message's turn: after its `talk`, the next turn; after a lone `user` (a crash between the two
/// appends), that turn's `talk` only.
pub fn new_messages(mem: &Mem, lines: &[&str]) -> Vec<Message> {
    let (start, skip_user) = match mem.root.last() {
        None => (0, false),
        Some(m) if m.kind == "user" => (m.t, true),
        Some(m) => (m.t + 1, false),
    };
    let mut out: Vec<Message> = Vec::new();
    for (t, line) in lines.iter().enumerate().skip(start) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let next = mem.root.len() + out.len();
        let mut messages = turn_messages(&value, next, t);
        if skip_user && t == start {
            messages.retain(|m| m.kind == "talk");
            if let Some(m) = messages.first_mut() {
                m.i = next;
            }
        }
        out.extend(messages);
    }
    out
}

pub fn should_recycle(session_bytes: usize, budget: usize, status: &str, queue_empty: bool, settled: bool) -> bool {
    session_bytes > 2 * budget && matches!(status, "idle" | "done") && queue_empty && settled
}

/// One value per line; a line that does not parse (torn by a crash) is skipped.
pub fn parse_lines<T: DeserializeOwned>(text: &str) -> Vec<T> {
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

pub fn chat_dir(slug: &str) -> PathBuf {
    home::bot_dir(slug).join("chat")
}

/// Append one line and fsync before returning (spec §2: a crash loses nothing written).
pub fn append_line<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(format!("{}\n", serde_json::to_string(value)?).as_bytes())?;
    file.sync_data()?;
    Ok(())
}

/// The bot's memory from disk. `repair` (daemon only) adds a missing final newline so the
/// next append starts its own line; MCP processes load with `repair = false` and never write.
pub fn load(slug: &str, budget: usize, repair: bool) -> Mem {
    let read = |name: &str| {
        let path = chat_dir(slug).join(name);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if repair
            && !text.is_empty()
            && !text.ends_with('\n')
            && let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&path)
        {
            let _ = f.write_all(b"\n");
        }
        text
    };
    fold(parse_lines(&read("main.jsonl")), parse_lines(&read("tree.jsonl")), budget)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxEntry {
    pub run: String,
    pub text: String,
    pub at: i64,
}

fn inbox_path(slug: &str) -> PathBuf {
    home::root().join(".state").join("memory").join(format!("{slug}.nodes.jsonl"))
}

/// write_node, from an MCP process: append only; the daemon validates and builds.
pub fn push_inbox(slug: &str, run: &str, text: &str) -> Result<()> {
    append_line(&inbox_path(slug), &InboxEntry { run: run.into(), text: text.into(), at: home::now() })
}

pub fn drain_inbox(slug: &str) -> Result<Vec<InboxEntry>> {
    let path = inbox_path(slug);
    let taken = path.with_extension("taking");
    if std::fs::rename(&path, &taken).is_err() {
        return Ok(Vec::new());
    }
    let entries = parse_lines(&std::fs::read_to_string(&taken)?);
    std::fs::remove_file(taken)?;
    Ok(entries)
}

pub fn budget(bot: &home::Bot) -> usize {
    bot.memory_view_bytes.unwrap_or(DEFAULT_VIEW)
}

/// VIEW_DOC and the rendered view, for a memory bot's main-agent instructions.
pub fn view_block(slug: &str, bot: &home::Bot) -> Option<String> {
    bot.memory.then(|| format!("{VIEW_DOC}\n\n{}", load(slug, budget(bot), false).render()))
}

pub fn compact_preamble(bot_name: &str) -> String {
    COMPACT.replace("{bot}", bot_name)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub l: u32,
    pub i: usize,
    pub tries: Vec<String>,
    pub run: String,
}

/// The daemon's per-bot memory state. A cache: everything in it can be recomputed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub session_bytes: usize,
    /// The main agent's pane when `session_bytes` started counting; a new pane is a new session.
    pub session_pane: String,
    pub last_log_at: i64,
    pub job: Option<Job>,
    /// Bytes of job prompts sent to the current compactor run.
    pub run_bytes: usize,
    /// (level, index, retry after).
    pub failed: Vec<(u32, usize, i64)>,
}

fn state_path(slug: &str) -> PathBuf {
    home::root().join(".state").join("memory").join(format!("{slug}.json"))
}

pub fn load_state(slug: &str) -> State {
    std::fs::read_to_string(state_path(slug)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_state(slug: &str, state: &State) -> Result<()> {
    std::fs::create_dir_all(state_path(slug).parent().unwrap())?;
    std::fs::write(state_path(slug), serde_json::to_vec_pretty(state)?)?;
    Ok(())
}

async fn close_compactor(herdr: &Client, mut run: home::Run) -> Result<()> {
    let _ = herdr.pane_close(&run.pane).await;
    run.status = "done".into();
    run.result = Some("memory compactor finished".into());
    run.pane_closed = true;
    home::save_run(&run)
}

/// One daemon pass over every memory bot: import turns, take compactor answers, hand out the
/// next job (starting or ending a compactor as needed), and recycle a long session.
pub async fn tick(
    herdr: &Client,
    mems: &mut HashMap<String, Mem>,
    contexts: &mut HashMap<String, Vec<String>>,
) -> Result<()> {
    let max = home::memory_config().max_compactors;
    let compactors = |runs: &[home::Run]| runs.iter().filter(|r| r.class == "compact" && r.status == "running" && !r.pane_closed).count();
    let mut running = compactors(&home::list_runs());
    let now = home::now();
    for slug in home::list_bots() {
        let Ok(bot) = home::load_bot(&slug) else { continue };
        if !bot.memory {
            continue;
        }
        let budget = budget(&bot);
        let mem = mems.entry(slug.clone()).or_insert_with(|| load(&slug, budget, true));
        let mut st = load_state(&slug);

        // 1. New turns → log messages (the cursor is derived from the log; see new_messages).
        let turns = std::fs::read_to_string(home::bot_dir(&slug).join("turns.jsonl")).unwrap_or_default();
        let lines: Vec<&str> = turns.lines().collect();
        for m in new_messages(mem, &lines) {
            append_line(&chat_dir(&slug).join("main.jsonl"), &m)?;
            st.session_bytes += m.size;
            st.last_log_at = now;
            mem.append(m);
        }

        // 2. A new main-agent pane is a new session.
        if let Some(bs) = home::load_state(&slug)
            && bs.root_pane != st.session_pane
        {
            st.session_pane = bs.root_pane;
            st.session_bytes = 0;
        }

        // 3. The compactor run, and a job whose run is gone.
        let compactor = home::list_runs()
            .into_iter()
            .find(|r| r.bot == slug && r.class == "compact" && r.status == "running" && !r.pane_closed);
        if let Some(job) = &st.job
            && compactor.as_ref().is_none_or(|r| r.id != job.run)
        {
            st.failed.push((job.l, job.i, now + RETRY_SECS));
            st.job = None;
        }

        // 4. Answers.
        for entry in drain_inbox(&slug)? {
            let Some(job) = st.job.as_mut() else { continue };
            if entry.run != job.run {
                continue;
            }
            job.tries.push(entry.text);
            match check(&job.tries) {
                Verdict::Accept(text) => {
                    let key = (job.l, job.i);
                    append_line(&chat_dir(&slug).join("tree.jsonl"), &Node { l: key.0, i: key.1, size: text.len(), text: text.clone() })?;
                    mem.insert(key, text);
                    st.failed.retain(|&(l, i, _)| (l, i) != key);
                    st.job = None;
                }
                Verdict::Retry(reply) => {
                    if let Some(r) = &compactor {
                        home::enqueue(&r.agent, &reply)?;
                    }
                }
                Verdict::Fail => {
                    st.failed.push((job.l, job.i, now + RETRY_SECS));
                    st.job = None;
                }
            }
        }

        // 5. The next job.
        let failed: HashMap<Key, i64> = st.failed.iter().map(|&(l, i, t)| ((l, i), t)).collect();
        let next = mem.due(&failed, now);
        let recycle_waiting = st.session_bytes > 2 * budget;
        if st.job.is_none() {
            match (next, compactor) {
                (Some(key), Some(r)) if st.run_bytes <= 2 * budget => {
                    let context = mem.context_lines(Mem::job_end(key));
                    let new = delta(contexts.get(&slug).map(Vec::as_slice).unwrap_or_default(), &context);
                    let prompt = mem.job_prompt(key, &new, false);
                    home::enqueue(&r.agent, &prompt)?;
                    st.run_bytes += prompt.len();
                    contexts.insert(slug.clone(), context);
                    st.job = Some(Job { l: key.0, i: key.1, tries: Vec::new(), run: r.id });
                }
                (Some(_), Some(r)) => {
                    // Its session has grown past the budget: a fresh compactor takes over.
                    close_compactor(herdr, r).await?;
                    contexts.remove(&slug);
                }
                // A bot that is down stays down: spawn would bring it up.
                (Some(key), None)
                    if running < max
                        && home::load_state(&slug).is_some()
                        && (now - st.last_log_at >= QUIET_SECS || recycle_waiting) =>
                {
                    let context = mem.context_lines(Mem::job_end(key));
                    let prompt = mem.job_prompt(key, &context, true);
                    let r = crate::run::spawn(herdr, &slug, "herdbot-memory", "compact", None, "memory compactor", &prompt).await?;
                    if r.status == "running" {
                        running += 1;
                        st.run_bytes = prompt.len();
                        contexts.insert(slug.clone(), context);
                        st.job = Some(Job { l: key.0, i: key.1, tries: Vec::new(), run: r.id });
                    }
                }
                (None, Some(r)) if matches!(crate::run::status(herdr, &r.agent).await.as_deref(), Some("idle") | Some("done")) => {
                    close_compactor(herdr, r).await?;
                    contexts.remove(&slug);
                }
                _ => {}
            }
        }

        // 6. Recycle a long session onto a fresh one carrying the view.
        if let Some(bs) = home::load_state(&slug) {
            let status = crate::run::status(herdr, &bs.agent).await.unwrap_or_default();
            let queue_empty = !home::queue_dir().join(format!("{}.jsonl", bs.agent)).exists();
            if should_recycle(st.session_bytes, budget, &status, queue_empty, mem.settled()) {
                match crate::run::restart(herdr, &slug, bs).await {
                    Ok(new) => {
                        println!("memory: recycled {slug} onto {}", new.route);
                        st.session_pane = new.root_pane;
                        st.session_bytes = 0;
                    }
                    Err(e) => eprintln!("memory: {slug}: recycle failed: {e}"),
                }
            }
        }
        save_state(&slug, &st)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn short(n: usize) -> Vec<Message> {
        (0..n).map(|i| Message::new(i, if i % 2 == 0 { "user" } else { "talk" }, &format!("m{i}"), 0)).collect()
    }

    fn long(n: usize) -> Vec<Message> {
        (0..n).map(|i| Message::new(i, "user", &"x".repeat(600), 0)).collect()
    }

    fn tiles(mem: &Mem) {
        let mut next = 0;
        for &k in &mem.view {
            assert_eq!(span(k).0, next, "view parts must tile the log in order");
            next += span(k).1;
        }
        assert_eq!(next, mem.root.len());
    }

    #[test]
    fn scale_line_is_exactly_node_bytes() {
        assert_eq!(SCALE.len(), NODE);
        assert!(COMPACT.contains("{bot}") && COMPACT.contains("write_node"));
        assert!(VIEW_DOC.contains("zoom(id, n)"));
    }

    #[test]
    fn short_messages_are_free_nodes_and_merge() {
        let mem = fold(short(2), vec![], 1000);
        assert_eq!(mem.tree[&(0, 0)], "user: m0");
        assert_eq!(mem.tree[&(1, 0)], "user: m0\ntalk: m1");
        assert!(mem.settled());
    }

    #[test]
    fn fold_tiles_fits_and_never_splits() {
        let mut mem = Mem::new(60);
        let mut previous: Vec<Key> = Vec::new();
        for m in short(64) {
            mem.append(m);
            tiles(&mem);
            // Never split: every old part lies inside one new part.
            for &old in &previous {
                let (s, n) = span(old);
                assert!(mem.view.iter().any(|&k| { let (ks, kn) = span(k); ks <= s && s + n <= ks + kn }));
            }
            previous = mem.view.clone();
        }
        // Free merges only concatenate, so the view merged but cannot drop under the budget.
        assert!(mem.view.len() < 64);
        // Older lines never cover fewer messages than recent ones.
        assert!(span(mem.view[0]).1 >= span(*mem.view.last().unwrap()).1);
        // Folding the same log again gives the same view.
        assert_eq!(fold(short(64), vec![], 60).view, mem.view);
    }

    #[test]
    fn compacted_view_fits_its_budget() {
        // Long messages need model nodes; answer every due job with a short line, the way a
        // compactor would, and the view must end within budget.
        let mut mem = Mem::new(200);
        let none = HashMap::new();
        for m in long(32) {
            mem.append(m);
            while let Some((l, i)) = mem.due(&none, 0) {
                mem.insert((l, i), format!("s{l}.{i} {}", "z".repeat(300)));
            }
            tiles(&mem);
        }
        assert!(mem.settled());
        assert!(mem.size() <= 200 + NODE, "size {}", mem.size());
        assert!(mem.view.len() < 32);
    }

    #[test]
    fn due_nodes_come_in_order() {
        let mut mem = fold(long(3), vec![], 100_000);
        let none = HashMap::new();
        assert_eq!(mem.due(&none, 0), Some((0, 0)));
        mem.insert((0, 0), "s".repeat(300));
        assert_eq!(mem.due(&none, 0), Some((0, 1)));
        mem.insert((0, 1), "t".repeat(300));
        // Message 2 next (one message at a time), and the 601-byte merge is due too.
        assert_eq!(mem.due(&none, 0), Some((0, 2)));
        mem.insert((0, 2), "u".repeat(300));
        assert_eq!(mem.due(&none, 0), Some((1, 0)));
        // A failed node waits for its retry time.
        let mut fresh = fold(long(2), vec![], 100_000);
        let failed = HashMap::from([((0, 0), 10)]);
        assert_eq!(fresh.due(&failed, 5), None);
        assert_eq!(fresh.due(&failed, 10), Some((0, 0)));
        fresh.insert((0, 0), "s".repeat(300));
        assert!(!fresh.settled());
    }

    #[test]
    fn renders_view_with_placeholders() {
        let mut messages = short(1);
        messages.extend(long(1).into_iter().map(|m| Message { i: 1, ..m }));
        let mem = fold(messages, vec![], 100_000);
        assert_eq!(mem.render(), "<chat>\n0+1|user: m0\n1+1|(not summarized yet: zoom it)\n</chat>");
    }

    #[test]
    fn job_prompts_carry_no_ids() {
        let mut messages = short(1);
        messages.extend(long(1).into_iter().map(|m| Message { i: 1, ..m }));
        let mem = fold(messages, vec![], 100_000);
        let end = Mem::job_end((0, 1));
        let context = mem.context_lines(end);
        assert_eq!(context, ["user: m0"]);
        let first = mem.job_prompt((0, 1), &context, true);
        assert!(first.starts_with("<chat>\nuser: m0\n</chat>"));
        assert!(first.contains(SCALE) && first.contains("Compress this message"));
        assert!(!first.contains("+1|") && !first.contains("0+"));
        let later = mem.job_prompt((0, 1), &[], false);
        assert!(!later.contains("<chat>"));
        assert_eq!(delta(&["a".into(), "b".into()], &["a".into(), "c".into(), "d".into()]), ["c", "d"]);
    }

    #[test]
    fn size_enforcement() {
        assert!(matches!(check(&["short line".into()]), Verdict::Accept(t) if t == "short line"));
        let Verdict::Retry(reply) = check(&["y".repeat(600)]) else { panic!("over limit retries") };
        assert!(reply.starts_with("That line is 600 bytes; the limit is 512."));
        assert!(reply.ends_with("| ← LIMIT"));
        let tries: Vec<String> = (0..5).map(|k| "y".repeat(600 - k * 20)).collect();
        assert!(matches!(check(&tries), Verdict::Accept(t) if t.len() == 520));
        assert!(matches!(check(&["  ".into()]), Verdict::Fail));
    }

    #[test]
    fn zoom_and_date() {
        let mem = fold(short(2), vec![], 1000);
        assert_eq!(mem.zoom(0, 1), "0+0|user: m0");
        assert_eq!(mem.zoom(0, 2), "0+1|user: m0\n1+1|talk: m1");
        assert_eq!(mem.zoom(1, 2), "No line 1+2.");
        assert_eq!(mem.zoom(0, 3), "No line 0+3.");
        assert_eq!(mem.zoom(4, 1), "No line 4+1.");
        assert!(mem.date(0).starts_with("19") || mem.date(0).starts_with("20")); // epoch 0, local zone
        assert_eq!(mem.date(9), "No message 9.");
    }

    #[test]
    fn imports_turns_and_decides_recycles() {
        let both = serde_json::json!({"at": 5, "prompt": "hi", "reply": "yo"});
        let m = turn_messages(&both, 3, 7);
        assert_eq!((m[0].i, m[0].kind.as_str(), m[1].i, m[1].kind.as_str()), (3, "user", 4, "talk"));
        assert_eq!(m[0].size, "user: hi".len());
        assert_eq!((m[0].t, m[1].t), (7, 7));
        let typed = serde_json::json!({"at": 5, "prompt": null, "reply": "yo"});
        assert_eq!(turn_messages(&typed, 0, 0).len(), 1);
        assert!(should_recycle(129, 64, "idle", true, true));
        assert!(!should_recycle(128, 64, "idle", true, true));
        assert!(!should_recycle(129, 64, "working", true, true));
        assert!(!should_recycle(129, 64, "idle", false, true));
        assert!(!should_recycle(129, 64, "done", true, false));
        let lines: Vec<Message> = parse_lines("{\"i\":0,\"kind\":\"user\",\"text\":\"a\",\"size\":7,\"date\":0}\n{\"i\":1,\"ki");
        assert_eq!(lines.len(), 1); // torn last line skipped
    }

    #[test]
    fn importing_twice_is_a_no_op() {
        let turns = [r#"{"at":1,"prompt":"a","reply":"b"}"#, r#"{"at":2,"prompt":null,"reply":"c"}"#];
        let mut mem = Mem::new(1000);
        let first = new_messages(&mem, &turns);
        assert_eq!(first.iter().map(|m| (m.i, m.kind.as_str(), m.t)).collect::<Vec<_>>(), [(0, "user", 0), (1, "talk", 0), (2, "talk", 1)]);
        for m in first {
            mem.append(m);
        }
        assert!(new_messages(&mem, &turns).is_empty());
        // A crash after logging only turn 0's prompt: only its reply is imported next time.
        let mut partial = Mem::new(1000);
        partial.append(new_messages(&partial, &turns[..1]).remove(0));
        let rest = new_messages(&partial, &turns);
        assert_eq!(rest.iter().map(|m| (m.i, m.kind.as_str(), m.t)).collect::<Vec<_>>(), [(1, "talk", 0), (2, "talk", 1)]);
    }
}
