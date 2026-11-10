<?php
/**
 * D · Assembly Order: genuinely sequential setup. Each step depends on the one before.
 * Each step carries an id so a reader can enter at step 4 from a link.
 */
$this->layout('partials::layout', [
    'title' => 'Getting started · Herdbot',
    'description' => 'Requirements, Homebrew install, herdbot doctor and setup, a first bot with herdbot new and up, how a task is routed, and the first task.',
    'path' => '/getting-started/',
    'notFound' => false,
]);
?>
<section class="intro intro--compact" aria-labelledby="guide-title">
    <p class="t-smallcaps">herdbot · assembly order</p>
    <h1 id="guide-title">Getting <span class="t-accent">started.</span></h1>
    <p class="t-lead">Seven steps: check the machine, install, let doctor and setup repair
        what they find, create a bot, learn the route, hand it a task.</p>
    <p class="t-meta guide-source">Commands below are quoted from the project README, its
        CLI usage text, and the Homebrew cask in <code>packaging/</code>. None of them were
        executed while building this page.</p>
</section>

<div class="assembly">
    <section class="assembly__step" id="step-1" aria-labelledby="step-1-title">
        <span class="assembly__num" aria-hidden="true">01</span>
        <h2 id="step-1-title" class="t-headline-sm">Check what herdbot needs</h2>
        <div class="prose">
            <p>herdbot is a macOS tool, and this is the part that does not move: the daemon
                runs under launchd. The cask declares macOS Sequoia or later.</p>
            <p>It drives other tools rather than replacing them. You need:</p>
            <ul>
                <li><a href="https://herdr.dev">herdr</a> running. herdbot was tested with
                    herdr 0.9.1, socket protocol 22.</li>
                <li>at least one harness CLI on your <code>PATH</code>, signed in:
                    <code>claude</code>, <code>codex</code>, <code>cursor-agent</code> or
                    <code>pi</code>. Pi is restricted to the <code>opencode-go</code>
                    provider — herdbot rejects any other pi model.</li>
                <li>Rust with edition 2024 support, only if you build from source. The cask
                    does not need it.</li>
            </ul>
            <p>Nothing else is required. There is no server to run and no database to
                install: the herdbot home is a directory of plain files.</p>
        </div>
    </section>

    <section class="assembly__step" id="step-2" aria-labelledby="step-2-title">
        <span class="assembly__num" aria-hidden="true">02</span>
        <h2 id="step-2-title" class="t-headline-sm">Install the app and the CLI</h2>
        <div class="prose">
            <pre><code>brew install --cask slogsdon/tap/herdbot</code></pre>
            <p>The cask is published from the
                <a href="https://github.com/slogsdon/homebrew-tap">slogsdon/homebrew-tap</a>
                repository — Homebrew’s <code>slogsdon/tap</code> shorthand names it. That tap
                is the distribution channel for Herdbot, not a second project: the cask
                installs <code>Herdbot.app</code> into Applications and links the binary
                bundled at <code>Herdbot.app/Contents/Helpers/herdbot</code> onto your
                <code>PATH</code>. Uninstalling the cask stops
                <code>dev.herdbot.daemon</code> and quits the menu bar app.</p>
            <p>The zip from
                <a href="https://github.com/slogsdon/herdbot/releases">GitHub Releases</a>
                works the same way: drag <code>Herdbot.app</code> to Applications, open it,
                and press <strong>Set up everything</strong>.</p>
            <p>Then pick one: open Herdbot from Applications and use its setup window, or run
                <code>herdbot setup</code> in a terminal. Both do the same work, and step 3
                covers the terminal path.</p>
        </div>
    </section>

    <section class="assembly__step" id="step-3" aria-labelledby="step-3-title">
        <span class="assembly__num" aria-hidden="true">03</span>
        <h2 id="step-3-title" class="t-headline-sm">Let doctor name the problems, and setup fix them</h2>
        <div class="prose">
            <p><code>herdbot doctor</code> is the first-run and health check, and it exits 1
                if any check fails. It looks at whether herdr answers, which harness CLIs are
                on <code>PATH</code>, whether the herdbot home and <code>herdbot.toml</code>
                exist, whether the launchd service is installed and runs the same binary you
                just invoked, whether the CLI is linked onto <code>PATH</code>, and whether
                the running daemon is older than the binary on disk.</p>
            <pre><code>herdbot doctor</code></pre>
            <p><code>herdbot doctor --json</code> prints the same checks for a script.</p>
            <p><code>herdbot setup</code> runs the fixes doctor names, once each, in order:
                <code>home</code>, <code>cli-link</code>, <code>service</code>,
                <code>restart-daemon</code>. <code>--only</code> picks one of those;
                <code>--yes</code> answers its prompts.</p>
            <pre><code>herdbot setup</code></pre>
            <p>What that does, in the README’s words: it checks herdr and the harness CLIs,
                creates <code>~/Bots</code> and a commented <code>herdbot.toml</code>, links
                the CLI, installs the launchd service, and offers a starter bot. Accept the
                starter-bot prompt to create <code>helper</code>, then edit its
                instructions:</p>
            <pre><code>$EDITOR ~/Bots/helper/BOT.md</code></pre>
            <p>The service writes
                <code>~/Library/LaunchAgents/dev.herdbot.daemon.plist</code> and starts the
                daemon at login. After upgrading the binary, doctor flags a daemon older than
                the file on disk; restart it with:</p>
            <pre><code>herdbot setup --only restart-daemon</code></pre>
            <p><code>herdbot service status</code>, <code>install</code> and
                <code>uninstall</code> manage the same plist by hand. Set
                <code>HERDBOT_HOME</code> to keep bots somewhere other than
                <code>~/Bots</code>.</p>
        </div>
    </section>

    <section class="assembly__step" id="step-4" aria-labelledby="step-4-title">
        <span class="assembly__num" aria-hidden="true">04</span>
        <h2 id="step-4-title" class="t-headline-sm">Create a bot and open its workspace</h2>
        <div class="prose">
            <p>A bot is a folder with a <code>bot.toml</code> and a <code>BOT.md</code>.
                <code>herdbot new</code> takes a name, and optionally a working directory and
                the task class its main agent is routed by (unset, it uses
                <code>default</code>):</p>
            <pre><code>herdbot new &lt;name&gt; [--cwd DIR] [--class CLASS]</code></pre>
            <p>A bot’s <code>cwd</code> defaults to its own folder; pointing it at a checkout
                is how you get a bot that works on a repository:</p>
            <pre><code>herdbot new herdbot-dev --cwd ~/Code/herdbot</code></pre>
            <p>It prints <code>created …/herdbot-dev (edit BOT.md for its instructions)</code>
                and leaves <code>BOT.md</code> empty for you to fill in. Then open the
                workspace in herdr:</p>
            <pre><code>herdbot up herdbot-dev</code></pre>
            <p><code>up</code> starts the bot’s main agent and prints the agent name,
                workspace id and root pane. It is not strictly required before the first
                message: the daemon brings a bot up when it finds work for it, and the
                workspace opens in herdr the first time a message arrives.
                <code>herdbot down &lt;bot&gt;</code> stops it again.</p>
            <p><code>herdbot list</code> shows every bot with its title, whether it is up, how
                many schedules are on, and whether it came from Grok Bot or was created
                locally.</p>
        </div>
    </section>

    <section class="assembly__step" id="step-5" aria-labelledby="step-5-title">
        <span class="assembly__num" aria-hidden="true">05</span>
        <h2 id="step-5-title" class="t-headline-sm">Know which route the task will take</h2>
        <div class="prose">
            <p>The routing table is <code>routing.toml</code> in the herdbot home, written
                from the repository default on first use and free to edit. Every candidate is
                <code>harness:model[:effort]</code>. For each task herdbot tries, in order:</p>
            <ol>
                <li>an explicit <code>model</code> argument on <code>task</code></li>
                <li>the bot’s override in <code>[bots.&lt;slug&gt;]</code></li>
                <li><code>[classes.&lt;subagent_type&gt;]</code>, an ordered list of
                    <code>harness:model[:effort]</code></li>
                <li><code>[default]</code></li>
            </ol>
            <p>The first available candidate wins. A harness is available when its binary is
                on <code>PATH</code> and it isn’t cooling down after a quota or auth failure.
                Every launched agent gets an explicit effort: the candidate’s own, then the
                class’s, then <code>[default]</code>, then <code>medium</code>.</p>
            <p>Task classes are Grok Bot’s subagent types (<code>explore</code>,
                <code>shell</code>, <code>bash</code>, <code>browser_use</code>,
                <code>computer_use</code>, <code>debug</code>,
                <code>media_review</code>, <code>custom</code>) plus <code>code</code>,
                <code>review</code>, <code>plan</code>, <code>research</code>, and
                <code>compact</code> for a memory bot’s compactor runs. Ask what a class
                resolves to right now:</p>
            <pre><code>herdbot route explore
herdbot route code --bot herdbot-dev</code></pre>
        </div>
        <div class="table-wrap">
            <table>
                <caption class="t-smallcaps">the four harnesses</caption>
                <thead>
                    <tr>
                        <th scope="col">Harness</th>
                        <th scope="col">Model and effort</th>
                        <th scope="col">How its instructions arrive</th>
                    </tr>
                </thead>
                <tbody>
                    <tr>
                        <th scope="row"><code>claude</code></th>
                        <td><code>--model</code>, <code>--effort</code></td>
                        <td><code>--append-system-prompt-file</code></td>
                    </tr>
                    <tr>
                        <th scope="row"><code>codex</code></th>
                        <td><code>-m</code>, <code>-c model_reasoning_effort=</code></td>
                        <td>its first prompt — the README found no flag to verify</td>
                    </tr>
                    <tr>
                        <th scope="row"><code>cursor-agent</code></th>
                        <td><code>--model</code>; effort is part of the id</td>
                        <td>its first prompt, plus a <code>herdbot</code> entry merged into
                            <code>&lt;cwd&gt;/.cursor/mcp.json</code></td>
                    </tr>
                    <tr>
                        <th scope="row"><code>pi</code></th>
                        <td><code>--model</code>, <code>--thinking</code>; provider forced to
                            <code>opencode-go</code></td>
                        <td><code>--append-system-prompt</code></td>
                    </tr>
                </tbody>
            </table>
        </div>
        <div class="prose">
            <p>The shipped table lists candidates for <code>claude</code>,
                <code>codex</code> and <code>pi</code> only. Its model IDs were listed by the
                installed CLIs on 2026-10-19, and which model suits which task is the author’s
                judgement — the table says so itself and expects tuning.</p>
            <p><strong>Cooldown and failover.</strong> When a subagent settles or blocks, and
                when a main agent finishes a turn, the daemon scans the last 15 non-empty
                lines of its pane for that harness’s failure text. A match writes
                <code>.state/cooldown/&lt;harness&gt;.json</code>, and routing skips that
                harness until the reset time plus a minute — or for an hour when the text
                gives no time. A subagent that failed this way is closed and its task re-queued
                on the next route. Deleting the cooldown file lifts it early.</p>
            <p>Those failure strings were checked against claude 2.1.304, codex 0.163.0 and
                cursor-agent 2026.10.16. Cursor’s quota text comes from its server and the
                pattern comes from one real capture, so other wordings may slip through.</p>
        </div>
    </section>

    <section class="assembly__step" id="step-6" aria-labelledby="step-6-title">
        <span class="assembly__num" aria-hidden="true">06</span>
        <h2 id="step-6-title" class="t-headline-sm">Give it a first task</h2>
        <div class="prose">
            <p>A message goes to the bot’s main agent. The daemon delivers it, so
                <code>send</code> needs the daemon running; a prompt sent to a busy agent waits
                in a queue instead of interrupting its turn.</p>
            <pre><code>herdbot send herdbot-dev "What does this repo do?"</code></pre>
            <p>A task starts a subagent in a new pane instead. The class decides the route:</p>
            <pre><code>herdbot task herdbot-dev explore "Which file defines the CLI usage string?"</code></pre>
            <p><code>task</code> returns the run id once the agent has taken the work, usually
                within a few seconds, and the pane appears to the right of the bot’s own. When
                the run finishes, the bot that asked gets the result — through
                <code>report_result</code>, through the harness’s turn-end hook, or from the
                last lines of the pane. A run that needs your approval is reported as blocked,
                once, with its pane.</p>
            <p>Watch it, or read it afterwards:</p>
            <pre><code>herdbot status --json
herdbot runs
herdbot history herdbot-dev --json --limit 5
herdbot focus herdbot-dev</code></pre>
            <p><strong>Approvals.</strong> The default is <code>"ask"</code>, which adds no
                flags and leaves each harness’s prompts alone. Only herdbot’s own tools are
                pre-approved. A bot can be switched to automatic review with
                <code>approvals = "auto"</code> in its <code>bot.toml</code>, or
                <code>[approvals] mode = "auto"</code> in <code>herdbot.toml</code> for every
                agent; full bypass flags (<code>--yolo</code>,
                <code>--dangerously-skip-permissions</code>) are never used. A prompt stuck
                behind a modal becomes a card:
                <code>herdbot card list</code>, then
                <code>herdbot card answer &lt;card-id&gt; &lt;option-id&gt;</code>, which
                re-checks the live screen before answering.</p>
            <p>If a bot should work on a repository rather than in its own folder, that is the
                <code>--cwd</code> from step 4. Schedules, rooms, memory and computer use all
                sit on top of this one loop; none of them are needed for a first bot.</p>
        </div>
    </section>

    <section class="assembly__step" id="step-7" aria-labelledby="step-7-title">
        <span class="assembly__num" aria-hidden="true">07</span>
        <h2 id="step-7-title" class="t-headline-sm">Expect these edges</h2>
        <div class="prose">
            <p>The README is unusually direct about what has and hasn’t been seen working, and
                these are its qualifications, kept as written.</p>
            <ul>
                <li>Early, and macOS only for now.</li>
                <li>Grok Bot’s schedules and skills are not imported, because Grok Bot keeps
                    them server-side. The roster import is one-way and writes nothing back.</li>
                <li><code>cursor-agent</code> launches, but its tools haven’t been seen working
                    end to end — the test account hit its weekly limit. Whether cursor asks for
                    each tool call under <code>--approve-mcps</code> is untested.</li>
                <li>Quota detection reads the pane, so claude’s quota detection is unverified;
                    only cursor’s has been seen working on a real limit.</li>
                <li>While a trust or warning modal is up, herdr can report the agent as idle and
                    a queued message is lost. When <code>fm</code> is available the daemon holds
                    the delivery instead, but the README’s author hasn’t seen that hold fire on a
                    real modal.</li>
                <li>The retry that re-sends a prompt when a subagent didn’t start a turn has
                    been seen once, with codex, and not reproduced, so that path is
                    untested.</li>
                <li>For computer use, the <code>ask</code> approval flow end to end and any
                    remote host are not verified.</li>
            </ul>
            <p>None of this is a reason to wait, as long as you read it the way it is written:
                a local tool, early, on one platform, whose author tells you which parts were
                watched live and which were not.</p>
        </div>
    </section>
</div>

<section class="page-section" aria-labelledby="next-title">
    <header class="index-head">
        <h2 id="next-title" class="t-smallcaps-lg">where to go</h2>
        <span class="t-dateline">after setup</span>
    </header>
    <ul class="link-list">
        <li>
            <a href="/herdbot/">Project introduction<span aria-hidden="true"> →</span></a>
            <p>What herdbot is, the router, and its status.</p>
        </li>
        <li>
            <a href="https://github.com/slogsdon/herdbot">Source on GitHub<span aria-hidden="true"> ↗</span></a>
            <p>The README these steps are quoted from, plus <code>routing.toml</code> and the
                full command list.</p>
        </li>
        <li>
            <a href="https://github.com/slogsdon/homebrew-tap">slogsdon/homebrew-tap<span aria-hidden="true"> ↗</span></a>
            <p>The tap behind <code>brew install --cask slogsdon/tap/herdbot</code>.</p>
        </li>
        <li>
            <a href="https://github.com/slogsdon/herdbot/releases">Releases<span aria-hidden="true"> ↗</span></a>
            <p>The signed zip, if you would rather install without Homebrew.</p>
        </li>
    </ul>
</section>
