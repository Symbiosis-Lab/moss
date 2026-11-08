/**
 * The bus between the shell frame and the editor frame: one end of the
 * `MessageChannel` the outer page made. Like the desktop's bus across its
 * webviews, a message reaches the frame that sent it too. A `BroadcastChannel`
 * would reach every tab on the origin, so a second tab would hear this one.
 */
import { inPageBus, type Bus } from '../ui/host';

interface Envelope { name: string; payload?: unknown }

export function portBus(port: MessagePort): Bus {
  const local = inPageBus();
  port.onmessage = (e: MessageEvent<Envelope>) => {
    if (typeof e.data?.name === 'string') local.emit(e.data.name, e.data.payload);
  };
  return {
    emit(name, payload) {
      local.emit(name, payload);
      port.postMessage({ name, payload } satisfies Envelope);
    },
    on: local.on,
  };
}

/** What the outer page posts to each frame, with one port transferred. */
export const PORT_MESSAGE = 'moss:bus-port';

/**
 * Resolves with the port the outer page sends this frame. Only a message from
 * the parent on this origin is taken.
 */
export function receivePort(): Promise<MessagePort> {
  return new Promise((resolve) => {
    const take = (e: MessageEvent) => {
      if (e.source !== window.parent || e.origin !== location.origin || e.data !== PORT_MESSAGE || !e.ports[0]) return;
      window.removeEventListener('message', take);
      resolve(e.ports[0]);
    };
    window.addEventListener('message', take);
  });
}
