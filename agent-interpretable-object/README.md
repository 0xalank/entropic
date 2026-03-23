# Agent Interpretable Object

This folder defines a self-descriptive object system for agents.

The intent is:

- efficient enough for direct agent use
- hierarchical enough to express real structure
- human interpretable enough to read without tooling
- extensible beyond office files into graphics, design, and scene formats

This is not a versioned JSON Schema system.

The definitions in this folder are the contract.
Agents should read them directly.

## Design Rules

1. Meaning comes from named definitions, not from opaque field conventions.
2. Every kind is expressed in terms of reusable primitives:
   - object
   - part
   - relation
   - view
   - action
   - invariant
   - capability
   - loss boundary
3. Every concrete kind is hierarchical.
4. Every concrete kind is sparse when possible.
5. Every concrete kind should be compositional.
6. Every concrete kind should declare what is expressible now and what is only preserved.

## Category-Theoretic Reading

This system is category-inspired in a practical sense.

- A `kind` is an object class.
- A concrete document/workbook/deck is an object instance.
- A `relation` is a morphism-like edge between parts.
- A `view` is a projection from the full object into a usable representation.
- An `action` is a structure-preserving transformation when valid.
- A family definition is reusable across many concrete kinds.

The goal is not abstract math for its own sake.
The goal is stable compositional semantics.

## Reading Order For Agents

1. Read [manifest.yaml](/Users/nova/agent/entropic/agent-interpretable-object/manifest.yaml).
2. Read [core/keywords.yaml](/Users/nova/agent/entropic/agent-interpretable-object/core/keywords.yaml).
3. Read [core/ontology.yaml](/Users/nova/agent/entropic/agent-interpretable-object/core/ontology.yaml).
4. Read [core/canonical-instance.yaml](/Users/nova/agent/entropic/agent-interpretable-object/core/canonical-instance.yaml).
5. Read the target kind definition under `kinds/`.
6. Read any family note under `families/` if the target kind extends it.

## Common Shape

Every definition file in this folder is expected to describe:

- `name`: stable identifier
- `purpose`: what the definition is for
- `keywords`: the important words agents should use
- `structure`: the main hierarchy
- `relations`: allowed meaningful edges
- `views`: useful projections
- `actions`: meaningful edits or transforms
- `invariants`: truths that should remain valid
- `capabilities`: what a runtime may actively support
- `loss_boundary`: what may be preserved without being fully editable

Concrete instance payloads should mirror this shape.

## Canonical Instances

The canonical runtime instance form should be:

- sparse
- path-addressable
- stable under `rg`
- compact enough for token-sensitive agent use

That means:

- definition files carry the meaning
- instance payloads carry the concrete object state
- instance payloads should reference definitions instead of repeating them

See [core/canonical-instance.yaml](/Users/nova/agent/entropic/agent-interpretable-object/core/canonical-instance.yaml).

## Why This Exists

The current office helper is useful, but it is still ad hoc.
This folder is the beginning of a direct-docs definition system so agents can interpret and manipulate objects without needing MCP-specific wrappers.
