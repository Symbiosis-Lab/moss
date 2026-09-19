---
lang: zh-hans
weight: 20
url: markdown
description: 用纯文本加少量符号写作，青苔把它排成网页。
uid: 3a1f7c20
translationKey: docs-editors
---
Markdown 格式是纯文本加上少量符号：`#` 是标题，`**` 是加粗，`-` 是列表。青苔读取标准 Markdown（CommonMark 与 GitHub 风格），并在此之上支持 Obsidian 风格的 [[引用文件与媒体|Wikilink 与嵌入]]、[[用短代码排版及插入特殊功能|短代码]]和标注。

因为是纯文本，你可以用任何编辑器书写，文件始终留在你自己的电脑上。

## 基础语法

在下方的真实编辑器中试试这些语法。示例已经填好，在你动手编辑之前不会自行播放。

<moss-ui-demo lang="zh-hans" static></moss-ui-demo>

需要完整的语法速查，可参考 [Markdown 语法速查表](https://www.markdownguide.org/basic-syntax/)，或 [Obsidian 的基础格式语法](https://help.obsidian.md/syntax)。

## 换行与新段落

Markdown 中，换行新起的一行文字在渲染之后与上一行仅隔一个空格。如果你想要新起一个段落，就在两段之间隔一空行。

在诗歌等文体的排版中，如果你想要新起一行、而中间不隔空行，你可以在上一行结尾处增加两个空格。

## 青苔的扩展

在标准 Markdown 之外，青苔还认得几种记号，各有专门的文档：

- **[[引用文件与媒体|Wikilink 与嵌入]]**：用 `[[双方括号]]` 链接和嵌入其他页面与媒体。
- **[[用短代码排版及插入特殊功能|短代码]]**：用 `:::` 围栏创建多栏、图库、大图等布局。
- **[[用 Frontmatter 定义页面|Frontmatter]]**：在文件顶部用 YAML 设置标题、日期、可见性等属性。

## 用什么写

青苔自带一个所见即所得的 Markdown 编辑器，右键文件夹选择「发布」即可开始。你也可以用任何顺手的编辑器，比如 [Obsidian](https://obsidian.md/zh/)、iA Writer、Typora 或 VS Code。青苔会监视文件夹，一保存就更新预览。
