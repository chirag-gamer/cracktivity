# Third-party notices

cracktivity's own source is MIT-licensed (see `LICENSE`). This file lists the
third-party assets that ship inside the app binary and the notices their
licenses require.

## Lucide icons (lucide-react)

- Source: https://github.com/lucide-icons/lucide
- License: ISC (icons derived from Feather are additionally MIT, Copyright
  2013-present Cole Bemis; both notices are reproduced in the upstream
  repository's LICENSE file)

```
ISC License

Copyright (c) 2026 Lucide Icons and Contributors

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR
IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

## AI models

Downloaded on demand, never bundled:

- `chirag-gamer/cracktivity-email-qwen3-0.6b` (LOW tier, D-048): the
  cracktivity project's own fine-tune, CC BY-NC-SA 4.0.
- Qwen / Phi-4 fallback pins: Apache-2.0 / MIT respectively, per their model
  cards on HuggingFace at the pinned revisions in
  `src-tauri/src/ai/models.rs`.

## Fonts

None bundled; the app uses the system font stack (Segoe UI Variable with
fallbacks), which is licensed with Windows itself.
