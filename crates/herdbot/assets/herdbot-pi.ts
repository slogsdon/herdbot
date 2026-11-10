/**
 * herdbot tools for pi. pi runs with --no-extensions -e builtin:mcp -e builtin:codemode (see route.rs), so this
 * extension only registers herdbot's stdio MCP server; pi's built-in MCP client does the rest.
 * Bot and run identity come from HERDBOT_BOT / HERDBOT_RUN in the pane environment.
 */

import { execFile } from "node:child_process";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

type Message = { role?: string; content?: unknown; stopReason?: string; errorMessage?: string };

/** The text of the last assistant message: its text items, without thinking or tool calls. */
function lastAssistantText(message?: Message): string {
	if (typeof message?.content === "string") return message.content;
	if (Array.isArray(message?.content)) {
		return message.content
			.filter((c): c is { type: "text"; text: string } => c?.type === "text" && typeof c.text === "string")
			.map((c) => c.text)
			.join("");
	}
	return "";
}

/** Hand one event to `herdbot hook pi` on stdin. Fire and forget: a hook never slows pi down. */
function hook(payload: object) {
	const child = execFile(process.env.HERDBOT_EXE ?? "herdbot", ["hook", "pi"], () => {});
	child.stdin?.end(JSON.stringify(payload));
}

export default function (pi: ExtensionAPI) {
	pi.on("agent_end", (event) => {
		const message = (event.messages as Message[]).findLast((message) => message.role === "assistant");
		hook({ event: "agent_end", text: lastAssistantText(message), stopReason: message?.stopReason, errorMessage: message?.errorMessage });
	});
	pi.on("ui_prompt_start", (event) => hook({ event: "ui_prompt_start", title: event.title ?? event.kind }));

	const args = ["mcp", "--bot", process.env.HERDBOT_BOT ?? ""];
	if (process.env.HERDBOT_RUN) args.push("--run", process.env.HERDBOT_RUN);
	pi.registerMcpServer("herdbot", {
		command: process.env.HERDBOT_EXE ?? "herdbot",
		args,
		env: { HERDBOT_HOME: process.env.HERDBOT_HOME ?? "" },
		// Discovered by searchTools() and called from code-mode scripts.
		exposure: "codemode",
		description: "herdbot: start subagents, message other bots, report results",
	});
}
