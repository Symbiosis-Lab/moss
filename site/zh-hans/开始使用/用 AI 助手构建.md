---
lang: zh-hans
title: 用 AI 助手构建
url: agents
uid: 275b3f41
weight: 65
description: 把网站文件夹和一条提示交给编程助手。青苔给它一份与你的青苔版本一致的手册，文件夹仍然属于你。
translationKey: docs-agents
---

## 开始

在你的网站文件夹里打开 AI 助手，粘贴：

```
用 `npm install -g @symbiosis-lab/moss` 安装青苔（moss）。阅读 `moss guide`，然后帮我在这个文件夹里构建并检查我的网站。
```

- **Claude Code：** 在文件夹里运行 `claude`。青苔构建过一次网站之后，运行 `moss agents init`：Claude Code 之后会自动阅读青苔的手册。
- **Codex（OpenAI）：** 在文件夹里运行 `codex`。
- **Cursor：** 打开文件夹，粘贴到 Agent。`moss agents init` 会添加一条 Cursor 自动读取的规则。
- **其他助手：** 粘贴上面的提示。如果它不读青苔的文件，就以「先阅读 `.moss/agents/SKILL.md`」开头。

## 为什么内容要和网站分开

青苔构建的是一个装着页面和图片的文件夹。代码是青苔和你的主题。助手也可以把整个网站直接写成代码；把两者分开，意味着：

- **改版不会动你的文字。** 页面是普通文件。换主题不会碰它们，升级青苔只有在你运行 `moss migrate` 时才会改动它们。
- **助手一次只改一样。** 它在 `.moss/theme/` 里改样式，不碰任何一句话；改页面，也不碰样式。
- **代码不会腐烂。** 青苔负责更新构建。某个版本停用了你的主题用到的 class，构建会告诉你现在该用什么（即将推出：`moss migrate` 替你改写）。
- **你的页面带得走。** 它们是 markdown 文件：能在任何编辑器里打开，放进 git，别的 markdown 工具也能读。
- **助手知道可以写哪里。** 页面和主题归你；`.moss/` 里的其余部分归青苔。

## `moss agents init`

在网站文件夹里运行：

```
moss agents init
```

它会写入：

- `.claude/skills/moss/SKILL.md`：已安装 Claude Code 时写入，Claude Code 会自动读取；
- `.cursor/rules/moss.mdc`：已安装 Cursor 时写入，Cursor 会自动读取；
- `.moss/agents/SKILL.md`：给其他任何助手；
- `.moss/AGENTS.md`：你网站自己的约定，由你或助手填写。

前三个文件都只指向 `moss guide`，别无其他。文件存在之后，每次构建会让它们保持最新，但不会创建它们。`moss agents sync` 会刷新它们，不碰 `.moss/AGENTS.md`。

## 助手的手册：`moss guide`

`moss guide` 输出青苔给助手的说明：项目如何组织、如何设计样式、要遵守的规则，以及如何确认改动有效。`moss guide --list` 列出所有主题；`moss guide all` 全部输出。

手册内置在你安装的青苔里，所以它描述的永远是这个版本，不需要网络。`moss describe --json` 按名称列出每个 class、token、front matter 字段和命令，供需要确切拼写的助手使用。

## 助手可以写哪里

- 页面、图片和文件夹：`.moss/` 之外的任何位置。
- 主题：`.moss/theme/`。
- 设置：`.moss/config.toml`。
- 网站的助手笔记：`.moss/AGENTS.md`。

`.moss/` 里的其余部分属于青苔：构建出的网站、版本历史、发布记录和网站密钥。它们只通过青苔命令修改，例如 `moss history` 或 `moss deploy`。

即将推出：`moss describe --json` 会列出每个路径及修改它的命令；`moss agents init` 会在文件夹顶层添加三行的 `AGENTS.md`，供 Codex、Gemini CLI、GitHub Copilot 等读取 `AGENTS.md` 的工具使用。

## 青苔更新时

某个版本重命名或停用了你的主题用到的 class 或 token 时，构建会指出它是哪一个、在哪个版本变的、现在该用什么。

`moss migrate` 替你改写文件夹。不加 `--apply` 时只以 diff 显示改动，不写入。加上 `--apply` 时先保存网站的一个版本，再写入。要撤销，用它给出的版本运行 `moss history --restore --at <版本> --yes`；文件夹在 git 里的话，也可以还原那次提交。目前它可以：

- 删除页面地址已经隐含的 `translationKey`；
- 把许多页面上重复的 class 块写成 front matter 字段（`--lift`）；
- 把导入页面的原始地址从 `syndicated:` 移到 `origin:`（`--import-sources`）。

即将推出：每次重命名都带有对应的改写。`moss migrate --list` 会列出文件夹里哪些已过时，有可更新内容时构建会在结尾给出一行提示，`moss migrate` 会改写页面和设置里被重命名的主题 class、token 与旧写法。

请助手运行 `moss migrate`，查看 diff，再加 `--apply` 运行并构建。文件夹在 git 里时，把迁移单独提交。

## 检查助手做了什么

助手的改动就是你文件夹里的文件。看预览；文件夹在 git 里时看 `git diff`。大改之前先保存一个版本：`moss history --save "改版之前"`；`moss history` 列出版本，`moss history --restore --at <版本> --yes` 还原其中一个。
