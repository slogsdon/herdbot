/**
 * herdbot tools for pi. pi runs with --no-extensions (see routing.ts), which also drops MCP,
 * so this extension exposes the same tools by shelling out to `herdbot tool <name>`.
 * Bot and run identity come from HERDBOT_BOT / HERDBOT_RUN in the pane environment.
 */

import { execFile } from "node:child_process";
import { Type } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

const HERDBOT = process.env.HERDBOT_EXE ?? "herdbot";

function call(name: string, params: unknown): Promise<string> {
	const args = ["tool", name, "--bot", process.env.HERDBOT_BOT ?? ""];
	if (process.env.HERDBOT_RUN) args.push("--run", process.env.HERDBOT_RUN);
	args.push(JSON.stringify(params));
	return new Promise((resolve) => {
		execFile(HERDBOT, args, { maxBuffer: 16 << 20 }, (error, stdout, stderr) => {
			resolve(error ? `error: ${stderr || error.message}` : stdout);
		});
	});
}

export default function (pi: ExtensionAPI) {
	const tools = JSON.parse(process.env.HERDBOT_TOOLS ?? "[]") as Array<{
		name: string;
		description: string;
		inputSchema: unknown;
	}>;
	for (const tool of tools) {
		pi.registerTool({
			name: tool.name,
			label: tool.name,
			description: tool.description,
			// JSON Schema straight from herdbot; TypeBox schemas are JSON Schema too.
			parameters: Type.Unsafe(tool.inputSchema as object),
			async execute(_toolCallId: string, params: unknown) {
				return { content: [{ type: "text", text: await call(tool.name, params) }], details: {} };
			},
		});
	}
}
