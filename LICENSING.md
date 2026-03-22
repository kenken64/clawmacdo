# Dual Licensing Model

## Overview

**[Project Name]** is available under a dual licensing model:

1. **Open Source License** — GNU General Public License v3.0 (GPLv3)
2. **Commercial License** — For organizations that cannot comply with the GPLv3 terms

---

## Option 1: GPLv3 (Free / Open Source)

You may use, modify, and distribute this software under the terms of the **GNU General Public License v3.0**.

This means:

- ✅ Free to use, modify, and distribute
- ✅ Access to full source code
- ✅ Community contributions welcome
- ⚠️ Any derivative work **must also be released under GPLv3**
- ⚠️ You **must disclose your source code** if you distribute the software
- ⚠️ You **must provide the same freedoms** to your users
- ⚠️ If you run a modified version on a server and users interact with it, you are **distributing** it

See `LICENSE-GPL.md` for the full GPLv3 license text.

### Who should use the GPLv3 license?

- Open source projects
- Academic and research use
- Personal projects
- Organizations comfortable with open-sourcing their derivative works

---

## Option 2: Commercial License (Paid)

If the GPLv3 terms do not work for your use case, you can purchase a **Commercial License** that grants you additional rights.

A commercial license is required if you want to:

- ✅ Use [Project Name] in proprietary / closed-source software
- ✅ Distribute [Project Name] without disclosing your source code
- ✅ Bundle [Project Name] in a commercial product or SaaS
- ✅ Remove the GPLv3 copyleft obligations
- ✅ Receive dedicated support and maintenance

See `LICENSE-COMMERCIAL.md` for the commercial license agreement template.

### Pricing

| Tier | Description | Price |
|------|-------------|-------|
| **Individual** | Solo developers, freelancers, annual revenue < $100K | $[XXX] / year |
| **Startup** | Companies with < 20 employees, annual revenue < $1M | $[X,XXX] / year |
| **Enterprise** | Larger organizations, unlimited developers | $[XX,XXX] / year |
| **OEM / Reseller** | Embedding in your own product for resale | Custom pricing |

For pricing inquiries, contact: **[your-email@example.com]**

---

## Quick Decision Guide

```
Do you plan to distribute or sell software that includes [Project Name]?
│
├─ NO → GPLv3 is fine (free)
│
├─ YES → Will your software also be open source under GPLv3?
│         │
│         ├─ YES → GPLv3 is fine (free)
│         │
│         └─ NO → You need a Commercial License (paid)
│
└─ UNSURE → Contact us at [your-email@example.com]
```

---

## Contributor License Agreement (CLA)

Contributors to [Project Name] are required to sign a **Contributor License Agreement** before their contributions can be merged. This ensures the Licensor retains the right to offer the software under both licenses.

By signing the CLA, you:

- Confirm you are the original author of the contribution
- Grant the Licensor a non-exclusive, royalty-free, worldwide license to use, modify, and sublicense your contribution
- Understand that your contribution will be available under both GPLv3 and the Commercial License

---

## How to Apply This Dual License

1. Place `LICENSE-GPL.md` in your repository root
2. Place `LICENSE-COMMERCIAL.md` in your repository root
3. Place this `LICENSING.md` file in your repository root
4. Add the following header to each source file:

```
Copyright (c) [Year] [Your Name / Company]

This software is licensed under a dual license model:

  1. GNU General Public License v3.0 — for open source use
     See LICENSE-GPL.md for details.

  2. Commercial License — for proprietary/commercial use
     See LICENSE-COMMERCIAL.md for details.

For licensing inquiries, contact: [your-email@example.com]
```

---

## FAQ

**Q: Can I use this in a SaaS product?**
A: If your SaaS code is open source under GPLv3, yes (free). If it's proprietary, you need a Commercial License.

**Q: Can I evaluate the software before purchasing?**
A: Yes. You can use the GPLv3 version for evaluation, development, and testing at no cost.

**Q: Do I need a license for internal tools?**
A: If the tool is used only internally and not distributed to third parties, GPLv3 generally permits this. However, if in doubt, contact us.

**Q: What happens if I modify the software?**
A: Under GPLv3, you must release your modifications under GPLv3 if you distribute them. Under the Commercial License, you may keep modifications proprietary.

---

**Note:** Customize all parameters in brackets before use. This document is a template and not legal advice. Consider consulting a legal professional for your jurisdiction.
