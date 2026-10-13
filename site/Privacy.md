---
nav: false
children: false
uid: 9f2c7a10
description: What moss collects, why, and your choices — in plain language.
lang: en
translationKey: privacy
---
moss is a local‑first tool for building and publishing websites. In one line: **your content stays on your own computer**, we collect as little data as we can, and there are **no ads and no tracking of individuals**.

This is an early policy. moss is in active development, and we'll keep refining it as the product grows. If anything here is unclear, [get in touch](#contact).

## Your content stays yours

Your folder and everything in it stays on your own device, including the website moss builds from it. Data only leaves your computer when you choose to publish; at that point moss uploads the site and its content to the server or platform you picked.

You can host the static site moss builds anywhere you like. When you do, moss's servers collect nothing about you (apart from [crash reports](#crash-reports)).

## Your account

If you publish to moss's servers, we use your email address and a public key moss generates for you automatically (an anonymous cryptographic ID). We store these on our server to know which sites are yours.

## Payments

Payments (hosting plans and domain purchases) are handled by Stripe, a payment processor in the US. Stripe holds your card details; we never see your card number. We keep only your Stripe customer ID and your email to manage your subscription, and Stripe sends your receipt.

## Custom domains

If you register a domain through moss, the registrar (OpenSRS / Tucows, in Canada) needs ICANN contact details: your name, address, phone, and country. We turn on WHOIS privacy so those details aren't published, and we don't store them ourselves, keeping only the domain name and your email. If you connect a domain you already own, we collect nothing extra.

## Newsletters

If you run a newsletter through moss, the email addresses your readers sign up with are stored for your site on our server, and newsletters are delivered through Resend (an email service in the US). We use confirmed opt‑in (double opt‑in) and don't store readers' IP addresses. As the site owner, your readers' data is your responsibility; we hold it on your behalf so you can email them.

## Comments

If you enable comments through moss, then when someone comments on your published site, their name (and optional email and website), their comment, and basic technical details (their IP address and browser) are stored in our own self‑hosted comment system, not a third party's. This helps us identify abusive comments. If reply notifications are enabled, those go out through Resend.

## Website analytics

For published sites, and for this site, we keep basic, aggregate traffic stats: the pages viewed, the referring page, country, browser and OS family, a screen‑size bucket (phone, tablet, or desktop), campaign tags in the link (utm_source and utm_medium), and named clicks on buttons a site marks for counting, such as a download button. These stats contain no cookies, no cross‑site tracking, no stored IP addresses, and nothing that identifies a person.

If you turn on the analytics dashboard, your site's stats sync from our server to your device and are kept locally, so you can analyze them on your own machine.

## Crash reports

If moss hits an error, it can send a crash report to our own server so we can fix the bug. It's the only thing moss reports automatically, and it does not track how you use the app. We strip personal data: your IP isn't stored (we mask it before it reaches our error tracker) and file paths are scrubbed. You can turn this off any time in **moss → Settings → Crash reporting**, and we delete crash reports after 90 days.

![[crash report.mp4|loop|100%|Toggle crash report]]

## Support logs

If you use **"Send Logs"** to report a problem, moss uploads your recent app log (scrubbed of personal data) and gives you a log number to quote when you tell us about the error, so we can fix it quickly. We delete these logs after 30 days.

![[send logs.mp4|loop|100%|Send logs manually]]


## Where your data is processed

Our server is in the **United States**, and some of the services we rely on (Stripe, Resend, Cloudflare) are US‑based; our domain registrar (OpenSRS / Tucows) is in **Canada**. If you're in the EU or UK, your data may be processed outside your country.

## How long we keep things

Crash reports: 90 days. Support logs: 30 days. Your account, payment, and subscriber records are kept while your account or site is active. You can contact us to remove your data sooner.

## Your choices and rights

You can ask us to see, correct, or delete your data, and you can turn off crash reporting at any time using the in‑app toggle.

## Contact

Questions, or want your data deleted? Email us at **hi@symbiosis-lab.org**.

## Changes

moss is early, and this policy will grow with it. We'll update this page when our practices change.

*Last updated: 2026‑07‑19.*
