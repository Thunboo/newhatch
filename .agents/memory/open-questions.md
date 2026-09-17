# Open Questions

Clarify these only when implementing the related area.

## Product And Data

- What exact identity should link a flag-bearing S2C response to the triggering player/request?
- Should HTTP/WebSocket detail expose explicit request/response boundaries and navigation?
- What does the `docs/todo.md` auto-removal note (`rows >= 5000 OR query time >= 100 ms`) govern?

## Capture And Reassembly

- Which WebSocket frame parser/representation should be used?
- What is the production policy for extremely long-lived flows and large TCP gaps?
- How should VLAN traffic and IPv6 extension headers be handled?
- What RX-ring size and `PACKET_MMAP` strategy are justified by benchmarks?

## Storage And Enrichment

- How should truncated segment tails and SQLite/file reconciliation recover after a crash?
- What Suricata correlation key and live filter synchronization mechanism should be used?
- Is immediate disk reclamation ever required beyond normal whole-segment retention?

## Transport Hardening

- Define collector PSK format, rotation, replay protection and whether encryption is required.
