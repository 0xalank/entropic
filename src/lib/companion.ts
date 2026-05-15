import { invoke } from "@tauri-apps/api/core";

export type CompanionFocusSnapshot = {
  supported: boolean;
  bundleId?: string | null;
  appName?: string | null;
  windowTitle?: string | null;
  pid?: number | null;
  focusedAtMs: number;
};

export type CompanionPermissionManifest = {
  id: string;
  label: string;
  description: string;
};

export type CompanionToolManifest = {
  id: string;
  name: string;
  description: string;
  inputSchema?: unknown;
};

export type CompanionTransportManifest = {
  kind: string;
  command?: string | null;
  args?: string[];
  port?: number | null;
};

export type CompanionRuntimeSkill = {
  id: string;
  name: string;
  bundleIds: string[];
  activation: string;
  transport: CompanionTransportManifest;
  contextMode: string;
  unitOfWork: string[];
  permissions: CompanionPermissionManifest[];
  tools: CompanionToolManifest[];
  systemPromptFragment: string;
  granted: boolean;
  status: string;
  statusReason?: string | null;
};

export type CompanionState = {
  focus?: CompanionFocusSnapshot | null;
  hostFocus?: CompanionFocusSnapshot | null;
  primarySkill?: CompanionRuntimeSkill | null;
  backgroundSkills: CompanionRuntimeSkill[];
  skills: CompanionRuntimeSkill[];
};

export type CompanionToolRunResult = {
  skillId: string;
  toolId: string;
  summary: string;
  output: unknown;
};

export async function getCompanionState(): Promise<CompanionState> {
  return invoke<CompanionState>("get_companion_state");
}

export async function setCompanionSkillGrant(
  skillId: string,
  granted: boolean,
): Promise<CompanionState> {
  return invoke<CompanionState>("set_companion_skill_grant", { skillId, granted });
}

export async function runCompanionTool(
  skillId: string,
  toolId: string,
  input: unknown = {},
): Promise<CompanionToolRunResult> {
  return invoke<CompanionToolRunResult>("companion_run_tool", {
    request: { skillId, toolId, input },
  });
}

export function formatCompanionFocusLabel(focus?: CompanionFocusSnapshot | null): string {
  if (!focus) return "No focused app";
  return focus.appName || focus.bundleId || "Unknown app";
}

export function buildCompanionContextPrompt(
  state: CompanionState | null,
  selectionOutput?: unknown,
): string | null {
  if (!state) return null;
  const host = state.hostFocus || state.focus;
  const skill = state.primarySkill;
  if (!host && !skill && selectionOutput === undefined) return null;

  const lines = [
    "Companion context for this turn:",
    host
      ? `- Host app: ${formatCompanionFocusLabel(host)}${host.bundleId ? ` (${host.bundleId})` : ""}.`
      : "- Host app: unknown.",
  ];

  if (host?.windowTitle) {
    lines.push(`- Host window: ${host.windowTitle}.`);
  }

  if (skill) {
    const toolDescriptions = skill.tools.map((tool) => {
      const schema =
        tool.inputSchema && typeof tool.inputSchema === "object"
          ? ` inputSchema=${JSON.stringify(tool.inputSchema)}`
          : "";
      return `${tool.id}: ${tool.description}${schema}`;
    });
    lines.push(
      `- Active Companion skill: ${skill.name} (${skill.status}).`,
      `- Unit-of-work grain: ${skill.unitOfWork.join(", ") || "not declared"}.`,
      `- Skill context mode: ${skill.contextMode}.`,
      `- Skill instruction: ${skill.systemPromptFragment}`,
    );
    if (!skill.granted) {
      lines.push("- Skill tools are not enabled yet. Ask for permission before reading or changing host app content.");
    } else {
      lines.push(
        `- Available Companion tools: ${toolDescriptions.join("; ") || "none"}.`,
      );
    }
  } else {
    lines.push("- Active Companion skill: none for the current focused app.");
  }

  const readyBackgroundSkills = state.backgroundSkills.filter(
    (backgroundSkill) => backgroundSkill.granted && backgroundSkill.status === "ready",
  );
  if (readyBackgroundSkills.length > 0) {
    lines.push(
      `- Ready background Companion skills: ${readyBackgroundSkills
        .map((backgroundSkill) => backgroundSkill.name)
        .join(", ")}.`,
    );
  }

  if (selectionOutput !== undefined) {
    lines.push(`- Fresh Companion selection probe for this turn: ${JSON.stringify(selectionOutput)}`);
  }

  lines.push(
    "Use the host app grain above. Keep proposed actions reviewable and scoped to the visible unit of work. Do not claim a whole document was inspected unless a Companion tool result explicitly says so.",
  );

  return lines.join("\n");
}
