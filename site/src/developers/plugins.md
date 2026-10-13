# Plugins

> **Planned.** Procera doesn't load third-party code yet. This page says what is planned so you can
> plan around it.

Procera is designed to be extended through **ports**: small, documented interfaces with the core
behind them, so an extension can't bypass permissions, auditing or workspace isolation.

## What's planned

- **Connectors** run outside the server, in a separate process with no database access. They receive
  work from Procera and return results through an authenticated callback, and all their outbound
  traffic goes through the same egress guard as webhooks. First-party connectors (HTTP, email,
  webhooks) come first; a connector SDK follows.
- **Process steps that call out** (an HTTP step in the Guided Studio) use connectors, with retries and
  results recorded on the request.
- **Templates and packs**: processes, forms and documents you can install into a workspace, reviewed
  like any other change before they publish.

## Today

Until plugins ship, integrate through the [API](docs-api.html) with personal API tokens, and through
[webhooks](docs-webhooks.html) when they land. Procera is open source (Apache-2.0): you can read and
change the code, and contributions are welcome.
