---
lang: zh-hant
translationKey: docs-editors
uid: 4b8d2f6e
weight: 20
url: markdown
description: 用純文字加少量符號寫作，青苔把它排成網頁。
---

Markdown 格式是純文字加上少量符號：`#` 是標題，`**` 是粗體，`-` 是列表。青苔讀取標準 Markdown（CommonMark 與 GitHub 風格），並在此之上支援 Obsidian 風格的 [[引用檔案與媒體|Wikilink 與嵌入]]、[[用短代碼排版及插入特殊功能|短代碼]]和標註。

因為是純文字，你可以用任何編輯器書寫，檔案始終留在你自己的電腦上。

## 基礎語法

在下方的真實編輯器中試試這些語法。範例已經填好，在你動手編輯之前不會自行播放。

<moss-ui-demo lang="zh-hant" static></moss-ui-demo>

需要完整的語法速查，可參考 [Markdown 語法速查表](https://www.markdownguide.org/basic-syntax/)，或 [Obsidian 的基礎格式語法](https://help.obsidian.md/syntax)。

## 換行與新段落

Markdown 中，換行新起的一行文字在渲染之後與上一行僅隔一個空格。如果你想要新起一個段落，就在兩段之間隔一空行。

在詩歌等文體的排版中，如果你想要新起一行、而中間不隔空行，你可以在上一行結尾處增加兩個空格。

## 青苔的擴充

在標準 Markdown 之外，青苔還認得幾種記號，各有專門的文件：

- **[[引用檔案與媒體|Wikilink 與嵌入]]**：用 `[[雙方括號]]` 連結和嵌入其他頁面與媒體。
- **[[用短代碼排版及插入特殊功能|短代碼]]**：用 `:::` 圍欄建立多欄、圖庫、大圖等版面。
- **[[用 Frontmatter 定義頁面|Frontmatter]]**：在檔案頂部用 YAML 設定標題、日期、可見性等屬性。

## 用什麼寫

青苔自帶一個所見即所得的 Markdown 編輯器，在資料夾上按右鍵選擇「發佈」即可開始。你也可以用任何順手的編輯器，比如 [Obsidian](https://obsidian.md/zh/)、iA Writer、Typora 或 VS Code。青苔會監看資料夾，一儲存就更新預覽。
