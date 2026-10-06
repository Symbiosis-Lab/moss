// A marker's own UI strings, per locale. Nothing else reads or writes this map — scene text
// (the sample Markdown a scene types) lives in scenes/*.json, not here.

const STRINGS = {
  en: {
    play: 'Play',
    stop: 'Stop',
    frame: 'Interactive moss editor demonstration',
    error: 'The interactive editor could not load',
  },
  'zh-hant': {
    play: '播放',
    stop: '停止',
    frame: '青苔編輯器互動示範',
    error: '無法載入互動編輯器',
  },
  'zh-hans': {
    play: '播放',
    stop: '停止',
    frame: '青苔编辑器交互演示',
    error: '无法加载交互编辑器',
  },
};

/** Looks up `key` for `locale`, falling back to `en`. */
export function string(locale, key) {
  return STRINGS[locale]?.[key] ?? STRINGS.en[key] ?? '';
}
