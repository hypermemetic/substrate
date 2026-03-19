# Solar

Demo activation showing nested plugin hierarchy. Models the solar system as a
tree of celestial bodies — star → planets → moons — each callable as a plugin.
Shows how to implement `ChildRouter` for hierarchical namespaces.

---

## Hub methods

**Namespace:** `solar`

| Method | Params | Returns |
|--------|--------|---------|
| `observe` | — | `System { star, planet_count, moon_count, total_bodies }` |
| `info` | `path: String` | `Body { name, body_type, mass_kg, radius_km, ... }` |

`path` supports nested lookup: `"earth"`, `"jupiter.io"`, `"saturn.titan"`.

---

## Nested routing

Each planet is a child plugin. Each planet's moons are children of that planet.
This means calls route hierarchically:

```
solar.earth.info
solar.jupiter.io.info
solar.saturn.titan.info
```

23 total bodies: 1 star, 8 planets, 14 moons.

---

## Notes

This activation is a reference implementation, not a production feature. See
`src/activations/solar/` for the `CelestialBody`, `ChildRouter`, and
`hub_methods(hub)` patterns.
