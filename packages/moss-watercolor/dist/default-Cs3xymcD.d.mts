//#region src/paper/default.d.ts
interface PaperOptions {
  width?: number;
  height?: number;
  seed?: number;
}
interface Paper {
  width: number;
  height: number;
  /** RGBA, one byte per channel; declared as a union so a future
   *  higher-precision generator can return Float32Array without moving
   *  this contract again. */
  data: Uint8Array | Float32Array;
}
declare function createPaper({
  width,
  height,
  seed
}?: PaperOptions): Paper;
//#endregion
export { PaperOptions as n, createPaper as r, Paper as t };