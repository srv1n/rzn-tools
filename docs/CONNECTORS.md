# Connector catalog

The current, code-checked catalog is [system/connectors.md](system/connectors.md).
It lists build profiles, canonical connector names, current tool groups, auth
boundaries, side effects, and known limits.

Use the running binary for exact schemas:

```bash
rzn-tools list
rzn-tools tools <connector> --output json
rzn-tools <connector> --help
```

Provider notes under [connectors/](connectors/) add setup details. If a
provider page differs from the live schema, use the live schema and file a
documentation fix.
