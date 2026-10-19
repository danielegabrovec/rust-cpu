Prima release pubblica di **RUST-CPU**, TUI Rust in italiano per hardware e LLM locali.

- Dieci viste responsive, temi, ricerca, ordinamento, grafici e navigazione da tastiera/mouse.
- Inventario CPU/CPUID, BIOS, scheda madre, moduli RAM e GPU; carico di sistema, processi, dischi e rete.
- Telemetria NVIDIA, sonde Ollama/LM Studio/llama.cpp, benchmark LLM esplicito e Capacity Lab.
- Microbenchmark CPU mix64 v1, snapshot JSON, registrazione CSV e comandi senza TUI.
- Installer Windows x64 per utente, portable ZIP, licenza MIT e documentazione dettagliata.

**Verifiche:** 56 test automatici, Clippy e formattazione superati; prove reali Windows,
NVIDIA, Ollama e LM Studio, terminale ConPTY e installazione/disinstallazione isolata.
Le immagini del README usano dati dimostrativi. I report personali non vengono pubblicati.

Scarica `*-setup.exe` oppure estrai `*-portable.zip` ed esegui `rust-cpu.exe`.
Confronta gli hash con `SHA256SUMS.txt`. L'installer non è firmato digitalmente.

Limiti: nessun driver kernel per SPD/timing/tensioni; GPU AMD/Intel in modalità inventario;
token di autenticazione dei server non gestiti. llama.cpp coperto da test di protocollo,
senza un server reale disponibile nella prova iniziale. Il benchmark CPU non usa punteggi CPU-Z.

Leggi [manuale e limiti](https://github.com/danielegabrovec/rust-cpu/blob/v0.1.0/docs/MANUALE.md)
e [verifiche dettagliate](https://github.com/danielegabrovec/rust-cpu/blob/v0.1.0/docs/VERIFICHE.md).
