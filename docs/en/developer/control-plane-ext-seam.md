# Control-plane extension seam
[简体中文](../../zh-CN/developer/control-plane-ext-seam.md) | English

The open-source control plane (`admin/`) is complete, but **not every deployment has every
capability**. Payment top-ups and SMS login exist only in private deployments. Their code
cannot live in the open-source repo — otherwise the public repo carries business code it
cannot use, and changing a private feature means touching a public repo.

The seam draws that boundary: **the open repo declares slots, the private repo fills them.**
One trait on the backend, one module on the frontend.

## 1. Backend: the `Ext` trait

Implemented in `admin/src/ext/mod.rs`.

```rust
pub trait Ext: Send + Sync {
    fn extra_permissions(&self) -> Vec<Permission> { vec![] }
    fn router(&self, state: &Shared) -> Router { Router::new() }
    fn public_router(&self) -> Router { Router::new() }
    fn migrations(&self) -> Vec<(i64, &'static str)> { vec![] }
}
```

`install()` stores the implementation in a `OnceLock`; `main.rs` assembles it once at startup:

```rust
ext::install(Box::new(EnterpriseExt::new(config)))?;   // NoExt in the open-source build
migrations::migrate(&store).await?;                     // includes extension tables
let app = app::build_app(state);                        // includes extension routes
```

| Method | Purpose |
|---|---|
| `extra_permissions` | Permission keys owned by the extension |
| `router` | Authenticated routes (`/api/admin/pay/*`) |
| `public_router` | Unauthenticated routes (payment callback, `/api/auth/sms/*`) |
| `migrations` | Extension tables, numbered from 9000 to stay clear of the core range |

The separate version range is deliberate: core versions keep growing, extension versions
stay in 9xxx, and upgrades never collide.

## 2. Frontend: the `@ext` module

Implemented in `admin/web/src/ext.ts`. In the open-source build it is **entirely empty**.

```ts
export interface AccountAction {          // action button next to the balance
  key: string
  labelKey: string                        // i18n key; the extension supplies the text
  component: () => Promise<{ default: Component }>
  available?: () => Promise<boolean>      // false hides the button
}

export interface LoginMethod {            // extra way to sign in
  key: string
  labelKey: string
  component: () => Promise<{ default: Component }>
  available?: () => Promise<boolean>
}

export const messages = {}                // merged into i18n
export const ledgerKindKeys = {}          // ledger kind → i18n key
export const ACCOUNT_ACTIONS = []
export const LOGIN_METHODS = []
```

| Consumer | Behaviour |
|---|---|
| `pages/me/Index.vue` | Renders `ACCOUNT_ACTIONS`, loads the dialog on click |
| `pages/Login.vue` | Renders `LOGIN_METHODS`; one method becomes a corner toggle, several become tabs |
| `i18n.ts` | Deep-merges `messages`; the extension only writes its own branches |
| `lib/ledger.ts` | Merges `ledgerKindKeys`; unknown kinds still render verbatim |

**Contract**: action and form components emit `close` to dismiss, `done` (actions) or
`success` (login) on completion. The host refreshes data or navigates accordingly.

`available` exists because capability is a runtime property, not a build-time one. A private
deployment without payments enabled should not show a top-up button at all — better hidden
than present and broken.

## 3. How a private repo plugs in

The private repo **does not copy open-source frontend code**. It adds its own modules and
swaps `@ext` at build time:

```ts
// private repo: web/vite.config.ts
const upstream = '../upstream/admin/web'

export default defineConfig({
  root: upstream,                    // build the open-source frontend directly
  resolve: {
    alias: {
      '@ext': './ext.ts',            // swap in the private implementation
      '@app': `${upstream}/src`,     // reuse api wrapper, store, types
    },
    // exactly one copy of each runtime library, or reactivity and i18n split
    dedupe: ['vue', 'vue-i18n', 'vue-router'],
  },
  build: { outDir: './dist' },
})
```

Deployment points `POINTER_ADMIN_STATIC_DIR` at the **private repo's `dist`**, not the
open-source one — the latter contains no payment or SMS UI at all.

## 4. Rules

- **No extension business code or copy in the open repo.** New UI goes in the private `ext.ts`.
- **No copying open-source code into the extension.** Import via `@app`.
- **Capability probes live in the extension.** The open repo does not know what "payment" or
  "SMS" means; it only asks what `available()` returns.
- Action components must not rely on host-provided props: cache what they need in the
  extension module (for example the channel remembered during the capability probe).

## 5. Verifying the boundary

```bash
cd admin/web && npm run build
grep -rl "pay/orders\|sms/login\|账户充值" dist/assets/*.js   # must print nothing
```

Any extension route or copy showing up in the open-source bundle means something leaked.
