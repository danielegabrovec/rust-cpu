**RUST-CPU0.2 — SQLite Session Lab**

- Archivio SQLite incluso nel binario: sessioni, campioni, benchmark, preferenze e avvisi.
- Caricamento/replay, confrontoA/B con medie, picchi, P95, delta e grafici normalizzati.
- Benchmark ripetuti, gruppi per modello/metodo/algoritmo/build e cronologia completa.
- Rinomina, cancellazione confermata, import/export, backup e controlli di integrità.
- GPU Windows AMD/Intel/NVIDIA con WDDM/DXGI, memoria dedicata/condivisa e dati per PID.
- Observer locale facoltativo e autenticato per richieste LLM instradate; nessun prompt/risposta salvato.
- Tredici viste, impostazioni persistenti, soglie configurabili e guida corretta.

110 test automatici su Windows, Clippy e build verificati. Prove reali: SQLite, terminale,
GPUAMD/NVIDIA, observerOllama e benchmark ripetuti CPU/LMStudio. Le schermate sono
dati dimostrativi; nessun report del PC viene pubblicato.

Scarica `*-setup.exe` o estrai `*-portable.zip`. SQLite non richiede installazione
separata. SHA-256 in `SHA256SUMS.txt`; installer non firmato digitalmente.
L'aggiornamento e la disinstallazione conservano l'archivio nel profilo dell'utente.

Limiti: SPD raw/timing e driver kernel non inclusi; sensoriCPU condizionati al
providerLibreHardwareMonitor già disponibile; percorsoIntel e llama.cpp non provati
fisicamente. Observer solo per le richieste instradate attraverso la sua porta.

[Guida sessioni e confronto](https://github.com/danielegabrovec/rust-cpu/blob/v0.2.0/docs/SESSIONI.md)
· [Verifiche](https://github.com/danielegabrovec/rust-cpu/blob/v0.2.0/docs/VERIFICHE.md)
· [Observer e autenticazione](https://github.com/danielegabrovec/rust-cpu/blob/v0.2.0/docs/OBSERVER.md)
