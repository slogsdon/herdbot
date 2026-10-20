/**
 * herdbot tools for pi. pi runs with --no-extensions -e builtin:mcp (see route.rs), so this
 * extension only registers herdbot's stdio MCP server; pi's built-in MCP client does the rest.
 * Bot and run identity come from HERDBOT_BOT / HERDBOT_RUN in the pane environment.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
	const args = ["mcp", "--bot", process.env.HERDBOT_BOT ?? ""];
	if (process.env.HERDBOT_RUN) args.push("--run", process.env.HERDBOT_RUN);
	pi.registerMcpServer("herdbot", {
		command: process.env.HERDBOT_EXE ?? "herdbot",
		args,
		env: { HERDBOT_HOME: process.env.HERDBOT_HOME ?? "" },
		// "direct": declared to the model at once. codemode is Task 3.
		exposure: "direct",
		description: "herdbot: start subagents, message other bots, report results",
	});
}
