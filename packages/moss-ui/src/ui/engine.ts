/**
 * The engine's commands for moss's own UI, over the transport the host
 * supplies (`host.engine`). Every host reaches the same command table.
 */
import { engineClient } from '@symbiosis-lab/moss-ui';
import { host } from "./host";

export const engine = engineClient((cmd, args) => host.engine.call(cmd, args));
