<?php
/**
 * F · Frontispiece: the single entry point to this project's set.
 * It names the work, states what it is and its status, and indexes what follows.
 * No figure: a frontispiece carries no argument.
 */
$this->layout('partials::layout', [
    'title' => 'Herdbot · a team of bots on your own machine',
    'description' => 'Runs persistent AI bots on your Mac in herdr workspaces and routes each task to claude, codex, cursor-agent or pi, failing over when a subscription hits its limit.',
    'path' => '/',
    'notFound' => false,
]);
?>
<section class="intro" aria-labelledby="project-title">
    <p class="t-smallcaps">shane logsdon · bots on herdr</p>
    <h1 id="project-title">A team of bots. <span class="t-accent">Your machine.</span></h1>
    <p class="t-lead">herdbot runs persistent AI bots locally, on the coding-agent
        subscriptions you already pay for. Each bot lives in its own herdr workspace, and a
        router decides which harness and model takes each task.</p>
    <a class="btn-arrow" href="/herdbot/getting-started/">Set up your first bot</a>
</section>

<section class="page-section" aria-labelledby="what-title">
    <header class="index-head">
        <h2 id="what-title" class="t-smallcaps-lg">what it is</h2>
        <span class="t-dateline">bots, panes, routes</span>
    </header>
    <div class="prose">
        <p>When a bot delegates work, the subagent opens in a pane next to it, so you can
            watch every agent and step in when one needs you. That is the whole point of
            running this locally instead of in a hosted fleet: the work stays visible.</p>
        <p>A router decides which harness (<code>claude</code>, <code>codex</code>,
            <code>cursor-agent</code> or <code>pi</code>) and which model takes each task.
            When one subscription hits its limit, the work moves to the next.</p>
        <dl class="facts">
            <div class="fact">
                <dt>bot</dt>
                <dd>A long-lived agent in its own herdr workspace, with its own instructions,
                    memory and skills folders, optional cron schedules, and its own
                    <code>bot.toml</code>.</dd>
            </div>
            <div class="fact">
                <dt>subagent</dt>
                <dd>A bot calls <code>task</code>; herdbot splits a pane to the right of the
                    bot and runs the agent there. The run finishes with
                    <code>report_result</code>, a harness turn-end hook, or a pane read, and
                    the parent gets the result.</dd>
            </div>
            <div class="fact">
                <dt>route</dt>
                <dd><code>harness:model[:effort]</code>. An explicit model wins, then the
                    bot’s override, then the task class’s ordered list, then the default.
                    The first available candidate takes the task.</dd>
            </div>
            <div class="fact">
                <dt>failover</dt>
                <dd>Quota or auth text in an agent’s pane writes a cooldown file, and routing
                    skips that harness until the reset time plus a minute.</dd>
            </div>
        </dl>
        <p>There is no orchestrator socket and no database. Every process reads and writes
            the same plain files under the herdbot home, and only the daemon prompts
            agents.</p>
    </div>
</section>

<section class="page-section" aria-labelledby="status-title">
    <header class="index-head">
        <h2 id="status-title" class="t-smallcaps-lg">status</h2>
        <span class="t-dateline">0.1.2 · 2026-10-22</span>
    </header>
    <div class="prose">
        <p><strong>This is early, and it is macOS only for now</strong> — the daemon runs
            under launchd. Version 0.1.2, MIT licensed, installed as a Homebrew cask or from
            a release zip.</p>
        <p>The source README’s list of what works, verified live on 2026-10-20:</p>
        <ul>
            <li>bots with their own workspace, instructions, memory and skills folders</li>
            <li>subagents in panes, with results delivered back to the bot that asked</li>
            <li>bot-to-bot messages, which wait until the receiving bot is idle</li>
            <li>cron schedules</li>
            <li>routing across claude, codex and pi, plus failover when a harness hits a
                quota or auth limit</li>
            <li>a one-way import of the Grok Bot roster</li>
            <li>rooms — group bots with channel, round-robin and moderated turn-taking</li>
            <li>computer use through cua</li>
        </ul>
        <p>What is not there yet: importing Grok Bot’s schedules and skills, because they
            aren’t stored on disk. <code>cursor-agent</code> launches, but its tools haven’t
            been seen working end to end, which is why it is absent from the routing list
            above. The menu bar app ships in <code>Herdbot.app</code> and is still being
            polished.</p>
        <p>This site adds no claim of its own. It is written from the project README at one
            revision; none of the product commands on the guide are executed here, and the
            guide says so.</p>
        <p>Publishing this site does not turn a local, single-machine tool into a supported
            release. The README’s verification dates are the README’s, not this site’s.</p>
    </div>
</section>

<section class="page-section" aria-labelledby="next-title">
    <header class="index-head">
        <h2 id="next-title" class="t-smallcaps-lg">where to go</h2>
        <span class="t-dateline">005 entries</span>
    </header>
    <ul class="link-list">
        <li>
            <a href="/herdbot/getting-started/">Getting started<span aria-hidden="true"> →</span></a>
            <p>Requirements, install, <code>herdbot doctor</code> and
                <code>herdbot setup</code>, a first bot, the route it takes, and the first
                task.</p>
        </li>
        <li>
            <a href="https://github.com/slogsdon/herdbot">Source on GitHub<span aria-hidden="true"> ↗</span></a>
            <p>The two crates, the CLI and daemon, the routing table, the menu bar app, and
                the release notes.</p>
        </li>
        <li>
            <a href="https://github.com/slogsdon/homebrew-tap">slogsdon/homebrew-tap<span aria-hidden="true"> ↗</span></a>
            <p>The tap the cask is published through — distribution, not a second site.
                <code>brew install --cask slogsdon/tap/herdbot</code> resolves here.</p>
        </li>
        <li>
            <a href="https://herdr.dev">herdr<span aria-hidden="true"> ↗</span></a>
            <p>The terminal workspace manager herdbot runs every bot inside. Installed
                separately; the README was tested against herdr 0.9.1, socket protocol
                22.</p>
        </li>
        <li>
            <a href="https://github.com/slogsdon/herdbot/releases">Releases<span aria-hidden="true"> ↗</span></a>
            <p>The signed zip for the current version, if you would rather not use
                Homebrew.</p>
        </li>
    </ul>
</section>
