import { ENGINE_PARAMS, ENGINE_TIER } from "./engine.generated.ts";
import type { EngineCommand, EngineCommands, EngineParams } from "./engine.generated.ts";

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

/** A command's answer, or the error string the engine gave. */
export type EngineResult<T> = { status: "ok"; data: T } | { status: "error"; error: string };

/** Carries one command to the engine; rejects with the engine's error string. */
export type EngineTransport = (cmd: EngineCommand, args: Record<string, unknown>) => Promise<unknown>;

type Camel<S extends string> = S extends `${infer Head}_${infer Tail}` ? `${Head}${Capitalize<Camel<Tail>>}` : S;

/** Every engine command as a camelCase method taking its arguments in order. */
export type EngineClient = {
  [C in EngineCommand as Camel<C>]: (...args: EngineParams[C]) => Promise<EngineResult<EngineOut<C>>>;
};

/**
 * The engine's commands over `transport`. An error string answers as
 * `{ status: "error" }`; anything thrown that is an `Error` is a fault in the
 * transport and propagates.
 */
export function engineClient(transport: EngineTransport): EngineClient {
  const client: Record<string, (...values: unknown[]) => Promise<EngineResult<unknown>>> = {};
  for (const cmd of Object.keys(ENGINE_PARAMS) as EngineCommand[]) {
    const names: readonly string[] = ENGINE_PARAMS[cmd];
    client[cmd.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase())] = async (...values) => {
      try {
        return { status: "ok", data: await transport(cmd, Object.fromEntries(names.map((n, i) => [n, values[i]]))) };
      } catch (e) {
        if (e instanceof Error) throw e;
        return { status: "error", error: e as string };
      }
    };
  }
  return client as unknown as EngineClient;
}
