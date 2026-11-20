---
lang: zh-hant
title: 用 AI 助理建置
url: agents
uid: a8bf9ae7
weight: 65
description: 把網站資料夾和一則提示交給程式助理。青苔給它一份與你的青苔版本一致的手冊，資料夾仍然屬於你。
translationKey: docs-agents
---

## 開始

在你的網站資料夾裡開啟 AI 助理，貼上：

```
用 `npm install -g @symbiosis-lab/moss` 安裝青苔（moss）。閱讀 `moss guide`，然後幫我在這個資料夾裡建置並檢查我的網站。
```

- **Claude Code：** 在資料夾裡執行 `claude`。青苔建置過一次網站之後，執行 `moss agents init`：Claude Code 之後會自動閱讀青苔的手冊。
- **Codex（OpenAI）：** 在資料夾裡執行 `codex`。
- **Cursor：** 開啟資料夾，貼到 Agent。`moss agents init` 會加入一條 Cursor 自動讀取的規則。
- **其他助理：** 貼上上面的提示。如果它不讀青苔的檔案，就以「先閱讀 `.moss/agents/SKILL.md`」開頭。

## 為什麼內容要和網站分開

青苔建置的是一個裝著頁面和圖片的資料夾。程式碼是青苔和你的主題。助理也可以把整個網站直接寫成程式碼；把兩者分開，意味著：

- **改版不會動你的文字。** 頁面是普通檔案。換主題不會碰它們，升級青苔只有在你執行 `moss migrate` 時才會改動它們。
- **助理一次只改一樣。** 它在 `.moss/theme/` 裡改樣式，不碰任何一句話；改頁面，也不碰樣式。
- **程式碼不會腐壞。** 青苔負責更新建置。某個版本停用了你的主題用到的 class，建置會告訴你現在該用什麼（即將推出：`moss migrate` 替你改寫）。
- **你的頁面帶得走。** 它們是 markdown 檔案：能在任何編輯器裡開啟，放進 git，別的 markdown 工具也能讀。
- **助理知道可以寫在哪裡。** 頁面和主題歸你；`.moss/` 裡的其餘部分歸青苔。

## `moss agents init`

在網站資料夾裡執行：

```
moss agents init
```

它會寫入：

- `.claude/skills/moss/SKILL.md`：已安裝 Claude Code 時寫入，Claude Code 會自動讀取；
- `.cursor/rules/moss.mdc`：已安裝 Cursor 時寫入，Cursor 會自動讀取；
- `.moss/agents/SKILL.md`：給其他任何助理；
- `.moss/AGENTS.md`：你網站自己的慣例，由你或助理填寫。

前三個檔案都只指向 `moss guide`，別無其他。檔案存在之後，每次建置會讓它們保持最新，但不會建立它們。`moss agents sync` 會更新它們，不動 `.moss/AGENTS.md`。

## 助理的手冊：`moss guide`

`moss guide` 輸出青苔給助理的說明：專案如何組織、如何設計樣式、要遵守的規則，以及如何確認變更有效。`moss guide --list` 列出所有主題；`moss guide all` 全部輸出。

手冊內建在你安裝的青苔裡，所以它描述的永遠是這個版本，不需要網路。`moss describe --json` 依名稱列出每個 class、token、front matter 欄位和指令，供需要確切拼寫的助理使用。

## 助理可以寫在哪裡

- 頁面、圖片和資料夾：`.moss/` 之外的任何位置。
- 主題：`.moss/theme/`。
- 設定：`.moss/config.toml`。
- 網站的助理筆記：`.moss/AGENTS.md`。

`.moss/` 裡的其餘部分屬於青苔：建置出的網站、版本歷史、發布記錄和網站金鑰。它們只透過青苔指令修改，例如 `moss history` 或 `moss deploy`。

即將推出：`moss describe --json` 會列出每個路徑及修改它的指令；`moss agents init` 會在資料夾頂層加入三行的 `AGENTS.md`，供 Codex、Gemini CLI、GitHub Copilot 等讀取 `AGENTS.md` 的工具使用。

## 青苔更新時

某個版本重新命名或停用了你的主題用到的 class 或 token 時，建置會指出是哪一個、在哪個版本變的、現在該用什麼。

`moss migrate` 替你改寫資料夾。不加 `--apply` 時只以 diff 顯示變更，不寫入。加上 `--apply` 時先儲存網站的一個版本，再寫入。要復原，用它給出的版本執行 `moss history --restore --at <版本> --yes`；資料夾在 git 裡的話，也可以還原那次提交。目前它可以：

- 刪除頁面位址已經隱含的 `translationKey`；
- 把許多頁面上重複的 class 區塊寫成 front matter 欄位（`--lift`）；
- 把匯入頁面的原始位址從 `syndicated:` 移到 `origin:`（`--import-sources`）。

即將推出：每次重新命名都附帶對應的改寫。`moss migrate --list` 會列出資料夾裡哪些已過時，有可更新內容時建置會在結尾給出一行提示，`moss migrate` 會改寫頁面和設定裡被重新命名的主題 class、token 與舊寫法。

請助理執行 `moss migrate`，查看 diff，再加 `--apply` 執行並建置。資料夾在 git 裡時，把遷移單獨提交。

## 檢查助理做了什麼

助理的變更就是你資料夾裡的檔案。看預覽；資料夾在 git 裡時看 `git diff`。大改之前先儲存一個版本：`moss history --save "改版之前"`；`moss history` 列出版本，`moss history --restore --at <版本> --yes` 還原其中一個。
