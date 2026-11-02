import { ENGINE_TIER } from "./engine.generated.ts";
import type { EngineCommand, EngineCommands } from "./engine.generated.ts";

export * from "./engine.generated.ts";

export type EngineArgs<C extends EngineCommand> = EngineCommands[C]["args"];
export type EngineOut<C extends EngineCommand> = EngineCommands[C]["out"];

/** The trust class of a command; a transport maps each to its own route. */
export type EngineTier = (typeof ENGINE_TIER)[EngineCommand];

/** Whether the engine serves `name`, as opposed to a host's own command. */
export function isEngineCommand(name: string): name is EngineCommand {
  return Object.prototype.hasOwnProperty.call(ENGINE_TIER, name);
}

/** `undefined` for a name the engine does not serve. */
export function engineTier(name: string): EngineTier | undefined {
  return isEngineCommand(name) ? ENGINE_TIER[name] : undefined;
}
