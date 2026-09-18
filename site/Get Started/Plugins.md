---
description: Write plugins in JavaScript that hook into moss's build pipeline and extend what moss and your site can do.
uid: 1bcf1f9b
title: Extend your site with plugins
translationKey: docs-extend
weight: 70
url: extend
---

Use a plugin when the site needs behavior that content, Markdown, and a theme cannot provide: process data, add generated page content, publish through another service, or syndicate work elsewhere. If you only need a different look, use [Design & themes](/get-started/design/) instead.

Plugins run as part of the build and publish pipeline. They can change what moss reads, contribute to generated pages, connect a deployment target, or send published work to another platform. A plugin may request privileged host capabilities, which moss presents for explicit approval.

Start from an existing registry plugin when one matches the job. Write your own when you need a service or transformation that is specific to your site, and test it against a copy of the folder before publishing.

The detailed [Write a plugin](/docs/extend/) guide covers the bundle and manifest contract. The [Hooks](/docs/reference/hooks/), [Slots](/docs/reference/slots/), [Manifest](/docs/reference/manifest/), and [CLI](/docs/reference/cli/) references define the exact interfaces.
