/**
 * Plugin message types for communication with moss
 */

/**
 * Messages that plugins can send to moss
 * @category Messages
 */
export type PluginMessage =
  | LogMessage
  | ProgressMessage
  | ErrorMessage
  | CompleteMessage;

/** @category Messages */
export interface LogMessage {
  type: "log";
  level: "log" | "warn" | "error";
  message: string;
}

/** @category Messages */
export interface ProgressMessage {
  type: "progress";
  phase: string;
  current: number;
  total: number;
  message?: string;
}

/** @category Messages */
export interface ErrorMessage {
  type: "error";
  error: string;
  context?: string;
  fatal: boolean;
}

/** @category Messages */
export interface CompleteMessage {
  type: "complete";
  success: boolean;
  error?: string;
  result?: unknown;
}
