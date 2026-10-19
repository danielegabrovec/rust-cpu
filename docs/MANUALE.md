# Manuale d'uso

## Primo avvio

Installa la release Windows x64 oppure estrai l'archivio portable in una cartella scrivibile.
Apri RUST-CPU dal menu Start o esegui `rust-cpu.exe` da Windows Terminal. Dopo il primo
campione compaiono CPU/RAM/processi; l'inventario WMI può richiedere alcuni secondi.
Un runtime LLM offline non pregiudica i dati hardware. Premi `?` per la guida.

Usa `Tab` per percorrere le viste. Su schermi ampi la sidebar consente clic diretti;
sui terminali compatti restano disponibili numeri e frecce. `Home` torna all'inizio,
`End` raggiunge gli ultimi elementi, `PgUp/PgDn` avanzano più velocemente.

## Durante un lavoro LLM

1. Avvia il server locale Ollama, LM Studio o llama.cpp e seleziona/carica il modello nel runtime.
2. Nella vista **LLM locali**, verifica stato e modelli. `F5` richiede una nuova sonda;
   l'aggiornamento automatico avviene ogni circa cinque secondi dopo l'ultima sonda.
3. Nella vista **Processi**, `/` filtra per nome o PID; `s` passa tra CPU/RAM/nome/PID,
   `a` cambia direzione e `Invio` apre il dettaglio. I processi marcati LLM sono
   riconosciuti dal nome, non dall'ispezione di prompt o righe di comando.
4. Premi `r` all'inizio del lavoro e di nuovo alla fine. La sessione CSV raccoglie
   misure aggregate, inclusi GPU0 e somma RSS dei processi riconosciuti.
5. Premi `e` per salvare l'inventario, il campione visualizzato e l'ultimo benchmark in JSON.

`Spazio` congela la vista: la raccolta e il CSV proseguono. Gli intervalli selezionabili
sono 500–10000 ms; `+` aumenta l'intervallo e riduce la frequenza dei campioni, `-` fa l'opposto.
La telemetria NVIDIA ha una propria cadenza di circa due secondi; non ogni frame è una nuova lettura GPU.
Il campo `sampling_ms` misura il costo della raccolta, non l'intervallo scelto tra due campioni.

## Benchmark

**LLM:** scegli un modello con le frecce nella relativa vista, premi `b`, leggi il riepilogo
e conferma con `Invio`. `Esc` annulla la conferma; `c` annulla un benchmark in corso.
Il prompt sintetico chiede tre frasi sull'organizzazione di una scrivania, con limite 64 token.
Il runtime potrebbe caricare il modello: il primo avvio può essere molto più lento del successivo.
La misura TTFT parte prima della richiesta e termina al primo frammento testuale non vuoto;
non equivale al solo tempo di calcolo. I dati mancanti restano `n/d`.

Le letture token/s native del runtime e quelle da usage/tempo complessivo hanno significati
diversi: il programma li distingue nella nota del risultato. Non confrontare direttamente
prove con contesto, modello, quantizzazione, cache, stato termico o runtime diversi.
Il programma non intercetta metriche delle generazioni avviate in altre applicazioni.

**CPU:** nella vista CPU premi `x` e conferma. Due fasi di circa 600 ms misurano il kernel
integer `mix64 v1`, prima su un worker, poi su un massimo di 16 worker logici. La misura
è in operazioni di mix al secondo: non è il punteggio CPU-Z né una misura generale di
prestazioni LLM. In debug non è confrontabile alla release. Premi `e` per salvare anche il risultato.

```powershell
.\rust-cpu.exe --cpu-benchmark --snapshot .\cpu-test.json --no-llm
.\rust-cpu.exe --benchmark-runtime ollama --benchmark-model MODELLO_INSTALLATO --snapshot .\llm-test.json
```

## Capacity Lab

Seleziona il parametro con `[` e `]`, modificalo con `+` e `-`:
parametri in miliardi, bit medi per peso, contesto, layer, teste KV e dimensione testa.
Il default rappresenta una configurazione ipotetica 8B, 4,5 bit, GQA 8 teste, 32 layer,
dimensione testa 128, contesto 8192. Non è il rilevamento automatico del modello scelto.

Formula: pesi = parametri × bit/8; KV FP16 = 2 × layer × contesto × teste KV × dimensione testa × 2 byte.
Il totale aggiunge il 15% dei pesi e 512 MiB di margine. La valutazione confronta
separatamente RAM libera e VRAM libera di una GPU; non somma capacità eterogenee.
MoE, architetture ibride, KV quantizzata, sliding window, batch, backend e ripartizione
CPU/GPU possono cambiare radicalmente l'occupazione. Verifica il modello reale nel runtime.

## Formati e posizione dei dati

JSON: `schema_version`, versione applicazione, data UTC, inventario, campione, runtime,
ultimo benchmark LLM ed eventuale benchmark CPU. CSV: timestamp, tempo sessione, costo raccolta,
CPU, RAM/swap, GPU0/VRAM/temperatura/potenza, conteggio e somma RSS dei processi LLM,
I/O disco e rete in byte/s. Celle vuote indicano sensore GPU assente.

Per impostazione iniziale i file interattivi sono in `Documents\RUST-CPU\exports` sotto
il profilo corrente; in cartelle Documenti reindirizzate scegli esplicitamente `--export-dir`.
I nomi contengono data/ora UTC. I file esistenti non vengono sovrascritti. L'esportazione
non invia dati a GitHub: report locali e test-output sono esclusi da Git.

## Risoluzione problemi

| Sintomo | Intervento |
|---|---|
| Messaggio "serve un terminale" | Apri Windows Terminal; per script usa `--snapshot` / `--record` |
| Caratteri o colori strani | Font monospaziato Unicode, terminale moderno, prova il tema `t` |
| Colonne tagliate | Allarga il terminale, usa scorrimento o una vista dedicata |
| CPU/RAM inizialmente a zero | Attendi primo campione e inventario iniziale |
| Sensori CPU n/d | Windows non li espone via sysinfo; non vengono installati driver kernel |
| GPU senza utilizzo | NVIDIA richiede nvidia-smi dal driver e accessibile in PATH; AMD/Intel sono inventario |
| Runtime offline | Verifica server e porta; F5; usa parametri URL per porte alternative |
| Runtime auth | Server con token: la release non gestisce credenziali; usa un server loopback accessibile o mantieni solo il monitor hardware |
| Modello elencato ma residenza ignota | Un elenco OpenAI compatibile non certifica che il modello sia caricato |
| Errore file già esistente | Scegli un nome nuovo; nessun overwrite automatico |
| CPU del processo diversa da altri tool | È normalizzata su tutti i thread logici, non su un singolo core |
| Benchmark annullato | `c` richiede cancellazione; eventuale lavoro già inviato può essere gestito autonomamente dal server |

## Disinstallazione

Dal menu Start scegli **Disinstalla** o usa Impostazioni → App. Per la versione portable,
elimina solo la cartella estratta quando il programma è chiuso. Le esportazioni personali
restano conservate. Non vengono modificati PATH, firewall, servizi o impostazioni BIOS.
L'installer non è firmato digitalmente: Windows può indicare editore non verificato.
