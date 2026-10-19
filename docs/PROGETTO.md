# RUST-CPU — Progetto e criteri di accettazione

## Estensione0.2 — Session Lab

Tredici viste: alle dieci iniziali si aggiungono Sessioni, Confronto e Impostazioni.
SQLite incorporato conserva dati, versioni/fonti, benchmark, preferenze e avvisi.
Registrazione e replay indipendenti, confronto A/B con statistiche complete e preview
limitate, ripetizioni benchmark e gruppi per metodo. Import/export e backup coerenti,
parametri SQL, schema riconoscibile e rifiuto di archivi futuri/estranei.

Estesi i provider Windows a WDDM/DXGI per AMD/Intel/NVIDIA e memoria per PID;
sensori LibreHardwareMonitor opzionali. Observer locale esplicito per richieste LLM
instradate, senza archiviare testo, con auth frontend/upstream separata.
Driver kernel, SPD raw e intercettazione trasparente non rientrano nei provider inclusi.

L'implementazione0.2 e i suoi limiti sono descritti in SESSIONI.md, HARDWARE.md,
OBSERVER.md e VERIFICHE.md. Il progetto iniziale seguente documenta la base0.1.

## Progetto iniziale0.1

Applicazione desktop a terminale, Rust nativo, prima piattaforma Windows x64.
Obiettivo: riunire inventario hardware di livello avanzato e osservabilità delle LLM locali.
Software indipendente, senza affiliazione a CPUID/CPU-Z. Non replica codice, marchi o benchmark proprietari.

## Prodotto

- Dieci viste coerenti con sidebar, scorciatoie, guida, tema scuro/chiaro, ridimensionamento.
- Dashboard CPU/RAM/GPU con storico limitato; processi filtrabili e ordinabili.
- CPU: topologia, CPUID, set istruzioni, cache, frequenza esposta dal sistema.
- Mainboard/BIOS e moduli RAM tramite WMI; nessun driver kernel installato.
- GPU: inventario Windows, telemetria NVIDIA opzionale tramite nvidia-smi.
- Dischi, spazio, I/O e rete; CPU/processi normalizzati alla capacità complessiva.
- Ollama, LM Studio e llama.cpp: rilevamento locale, modelli, residenza quando disponibile.
- Benchmark di generazione solo su comando esplicito; metriche misurate con provenienza.
- Capacity Lab: stima modificabile pesi + KV cache FP16 + margine; non promessa di compatibilità.
- Snapshot JSON e registrazione CSV. Nessuna telemetria esterna, pubblicità o account richiesto.
- Installer per utente, versione portable, checksum, licenza MIT, documentazione italiana.

## Architettura

Thread principale: eventi e rendering Ratatui/Crossterm. Worker separati per campioni di sistema,
WMI iniziale, sonde HTTP locali e benchmark. Stato a messaggi; nessuna chiamata lenta nel rendering.
Contratti in `src/model.rs`, raccolta in `collector`, runtime in `llm`, rendering in `ui`,
interazioni in `app`, esportazioni in `export`. CLI per snapshot e test senza terminale interattivo.

## Qualità e limiti dichiarati

Valori assenti restano assenti: CPU temperature/voltaggi/timing SPD dipendono da sensori e privilegi;
nessun valore stimato viene presentato come lettura hardware. Nessuna modifica BIOS/overclock.
Nessuna inferenza passiva di token/s di altri programmi: serve un benchmark diretto o metriche del runtime.
Nomi processo senza argomenti, senza utenti, hostname, seriali hardware o MAC nelle esportazioni.
I report locali restano esclusi da Git. Le risposte HTTP sono trattate come dati e limitate di dimensione.

## Gate di consegna

1. Compilazione, formattazione, Clippy e test di calcolo/protocolli/interazione/rendering.
2. Snapshot reale sul PC; nessuna fixture spacciata per telemetria reale.
3. Rendering a varie dimensioni e navigazione verificata.
4. Smoke test release e installazione/disinstallazione in cartella di prova.
5. Repository pubblica, CI Windows/Linux, release con installer e checksum.
6. Report finale con evidenze e funzioni non verificabili in assenza di runtime/hardware.
