# Verifiche — RUST-CPU 0.1.0

Data: **19 ottobre 2026**. Prima piattaforma provata fisicamente: Windows x64,
toolchain Rust/Cargo 1.99.0 MSVC, NSIS 3.11. I report hardware locali, le sessioni
e i log rimangono esclusi da Git. Le immagini pubbliche usano dati sintetici e
sono dichiarate come anteprime, non come risultati di misurazione.

## Controlli del codice

Superati `cargo fmt --all -- --check`, `cargo clippy --all-targets --locked -- -D warnings`,
`cargo test --all-targets --locked` e build release. La suite comprende:

- Rapporti, limiti delle percentuali, formule GQA/KV e storico limitato.
- Delta I/O, riuso dei contatori, CPU normalizzata e parser NVIDIA con assenze esplicite.
- Richieste HTTP a server loopback simulati: protocolli Ollama/LM Studio/OpenAI,
  NDJSON/SSE frammentati, Unicode, errori, auth, redirect rifiutati, limiti risposta,
  modelli remoti/encoder esclusi, contatori cache e cancellazione durante uno stream.
- Rendering di tutte le viste con dati assenti, nomi lunghi, temi, modali e dimensioni
  da terminale nullo a 160×48; navigazione End→Up, valori mancanti, identità del dettaglio processo.
- CLI avviata come processo reale: JSON, CSV e SVG insieme, file esistenti preservati,
  URL remoti rifiutati, opzioni incompatibili, help e avvio senza terminale.

La CI ripete i controlli su Windows e Linux e produce un pacchetto Windows.
Lo stato aggiornato della singola esecuzione è consultabile in
[GitHub Actions](https://github.com/danielegabrovec/rust-cpu/actions/workflows/ci.yml).
I risultati Linux di CI non equivalgono a una prova su un desktop Linux fisico.

## Dati reali e runtime

| Prova | Esito osservato |
|---|---|
| CPUID, identità CPU, core/thread, cache e istruzioni | Letture reali ottenute |
| WMI mainboard/BIOS e moduli RAM | Dati ottenuti senza amministratore |
| CPU, RAM, processi, dischi e interfacce | Campioni reali e serie CSV ottenuti |
| GPU NVIDIA | Utilizzo, VRAM, temperatura e potenza letti con nvidia-smi |
| GPU AMD integrata | Inventario presente; sensori dinamici n/d come previsto |
| Sonde Ollama e LM Studio | Server reali online; modelli ricevuti dalle API |
| Benchmark Ollama | Inferenza reale riuscita; token, decode nativo, prefill e TTFT ricevuti |
| Benchmark LM Studio | Inferenza reale riuscita; 64 token output, TTFT e throughput end-to-end |
| Benchmark CPU | Fasi single/multi completate nella release; risultati finiti e positivi |
| llama.cpp | Nessun server reale disponibile: offline; protocollo coperto da server di test |

Il benchmark Ollama è stato eseguito con un modello locale già installato di piccole
dimensioni; quello LM Studio con un modello testuale già disponibile. Non sono stati
scaricati modelli. Le misure non vengono presentate come confronto prestazionale
tra runtime, poiché modelli e condizioni erano diversi.

## Terminale reale e pacchetto

Eseguito smoke test ConPTY Windows a 80×24: apertura, caricamento dati, cambio scheda,
ricerca processo, dettaglio, esportazione, registrazione, pausa, tema, Capacity Lab,
benchmark CPU confermato, guida e uscita con ripristino del terminale. Una sessione
di 11 righe ha continuato a registrare mentre la vista era in pausa. Il report
esportato dalla TUI include il benchmark CPU terminato.

Installer e ZIP portable creati dallo script di packaging. Verificato il contenuto
dello ZIP. Il test isolato dell'installer ha controllato eseguibile, documentazione,
immagini, avvisi delle dipendenze, avvio `--version`, snapshot hardware reale,
disinstallazione e rimozione di chiavi/collegamenti del test. Le esportazioni
personali rimangono conservate. SHA-256 forniti insieme agli artefatti.

L'installer è **non firmato digitalmente**, come confermato dalla lettura Authenticode.
Non è stata simulata l'esperienza di SmartScreen su un PC nuovo. Il CRT MSVC è
collegato staticamente; non è stata provata una macchina Windows appena installata.

## Limiti ancora presenti

- Nessun driver kernel: SPD raw, timing RAM live, tensione CPU e sensori proprietari
  non sono implementati. I dati SMBIOS sono dichiarazioni firmware.
- GPU AMD/Intel: inventario, senza un provider dinamico dedicato. Multi-GPU NVIDIA
  gestita dal parser, ma non verificata fisicamente con più schede NVIDIA.
- Nessun token/s passivo delle generazioni di altre applicazioni e nessuna attribuzione
  completa della VRAM a singole conversazioni. Il CSV GPU usa il primo adattatore.
- API locali protette da token: stato auth riconosciuto, credenziali non gestite.
- Capacity Lab orientativo; CPU mix64 non equivalente al punteggio CPU-Z.
- macOS, Windows ARM64, Linux desktop fisico e altri hardware non certificati da questa prova.

Comandi ripetibili: [BUILD.md](BUILD.md), `scripts/verify.ps1 -Release` e
`scripts/test-installer.ps1`. I test di protocollo sono separati dalle prove reali
per non far dipendere la suite da modelli installati o server personali.
