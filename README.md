# Abigail - Your Personal AI Family Companion

**The private coordinator that manages your family's personal AI Entities.**

Abigail is the smart, private manager that lives in your home. You, the mentor or family head, create and customize individual **Entities** - each one a unique AI companion tailored to a person or purpose.

Your family talks directly to these Entities. Abigail coordinates everything behind the scenes: memory, skills, security, and growth.
Every install also provisions an immortal local coordinator identity called `Abigail Hive` that owns the shared `memory.db` and starts before user-facing Entities.
That Hive-owned store now runs on embedded SurrealDB, giving Abigail one local-first memory substrate for document records, graph links, semantic archives, and queue state without requiring Docker or an external database.

### Why Families Love Abigail
- **Complete privacy** - everything stays on your computer. No accounts with big tech companies.
- **One shared local memory core** - Abigail Hive owns a single embedded SurrealDB store and migrates supported legacy SQLite data on first launch.
- **Real power when you want it** - connect any Entity to the strongest cloud models with one click. You are never locked to one provider.
- **Provider switches stay safe** - model changes propagate across direct APIs and supported CLI tools without forcing families to rebuild shared Abigail data.
- **Grows with your family** - teach Entities new skills, memories, and preferences over time.
- **Simple for everyone** - kids, parents, and partners just chat naturally while you control the advanced settings.
- **Handles real-world sites safely** - for authenticated flows like webmail, Abigail uses Browser skill fallback instead of fragile IMAP/SMTP plumbing.

### Quick Start (5 minutes)
1. Install and open Abigail
2. Create your first Entity
3. Let your family start chatting with their own Entity
4. Connect any Entity to powerful cloud models when you want more capability

No accounts. No data sharing. Just your family and the Entities you control.

## Local Memory Model

- `Abigail Hive` owns the shared persistence root and opens the local `memory.db` store before user-facing Entities start.
- Shared orchestration state lives in the `abigail/hive` Surreal namespace/database pair.
- Per-Entity state is isolated into `abigail/entity_<uuid>` databases inside the same local store.
- Existing `abigail_seed.db`, `abigail_memory.db`, `jobs.db`, `calendar.db`, and `kb.db` files are treated as migration inputs only and are archived after a successful import.
- Browser, mentor-monitor, Id, Superego, and memory enrichment flows stay out-of-band so the family chat path remains responsive.

### For the Curious: Where Abigail Came From
Abigail began as part of a larger research vision for trustworthy, decentralized AI. We took the best parts of that vision and made them simple, safe, and useful for real families.

See [documents/Toward-a-Decentralized-Trust-Framework.pdf](documents/Toward-a-Decentralized-Trust-Framework.pdf).
