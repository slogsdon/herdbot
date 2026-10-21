//! OptChat-style memory for a bot: one endless log of its prompts and replies, a binary tree
//! of ≤512-byte summaries, and a fixed-size view folded from them. Design:
//! docs/superpowers/specs/2026-10-20-optchat-memory-design.md. The daemon is the only writer.

// Until the daemon uses memory (Task 4).
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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
}
