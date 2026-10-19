# Hardware, fonti delle misure e precisione

Questo documento descrive i provider implementati. I risultati osservati sul PC
di sviluppo e l'esito delle prove appartengono a [VERIFICHE.md](VERIFICHE.md):
una funzione implementata non dimostra da sola la disponibilità di ogni sensore
su qualsiasi hardware.

## GPU Windows: NVIDIA, AMD e Intel

Il provider Windows usa le API del sistema operativo, senza installare un nuovo
driver kernel:

- **DXGI** enumera gli adattatori hardware e ne legge il nome e la capacità di
  memoria dedicata dichiarata dal driver.
- **PDH**, con `PdhAddEnglishCounterW`, legge contatori WDDM usando percorsi
  inglesi indipendenti dalla lingua dell'interfaccia Windows.
- Le istanze PDH sono associate agli adattatori DXGI tramite il loro LUID. Questo
  identificatore resta interno e non viene aggiunto agli export.

Microsoft descrive la telemetria del Task Manager come dati del gestore memoria
e dello scheduler WDDM. Il requisito indicato è un driver **WDDM 2.0 o successivo**;
la disponibilità effettiva dei contatori dipende comunque da sistema e driver.
Fonte: [GPUs in the Task Manager, Microsoft DirectX](https://devblogs.microsoft.com/directx/gpus-in-the-task-manager/).

I contatori richiesti sono:

| Contatore PDH | Uso in RUST-CPU |
| --- | --- |
| `GPU Engine(*)\Utilization Percentage` | Carico adattatore e processo |
| `GPU Adapter Memory(*)\Dedicated Usage` | Memoria dedicata usata dall'adattatore |
| `GPU Adapter Memory(*)\Shared Usage` | RAM condivisa usata dall'adattatore |
| `GPU Process Memory(*)\Dedicated Usage` | Memoria dedicata attribuita a un PID |
| `GPU Process Memory(*)\Shared Usage` | RAM condivisa attribuita a un PID |

La lettura avviene su un worker dedicato, normalmente ogni secondo. Il primo
campione stabilisce la base dei contatori di carico; il successivo permette di
calcolare la percentuale. Una metrica non disponibile o non valida resta `null`
nel JSON e `N/D` nell'interfaccia: non viene convertita automaticamente in zero.

I getter usati dal collector non attendono le chiamate al driver. I campioni
WDDM più vecchi di cinque secondi vengono esclusi; l'inizializzazione oltre
quattro secondi viene segnalata. Una chiamata nativa già bloccata non viene
terminata forzatamente: resta isolata nel solo worker del provider. I worker sono
condivisi a livello di processo e sospendono nuove query quando non ci sono più
collector iscritti, evitando di creare nuovi thread a ogni collector.

Le API e il formato del buffer seguono la documentazione ufficiale
[PdhAddEnglishCounterW](https://learn.microsoft.com/en-us/windows/win32/api/pdh/nf-pdh-pdhaddenglishcounterw),
[PdhGetFormattedCounterArrayW](https://learn.microsoft.com/en-us/windows/win32/api/pdh/nf-pdh-pdhgetformattedcounterarrayw)
e [IDXGIAdapter::GetDesc](https://learn.microsoft.com/en-us/windows/win32/api/dxgi/nf-dxgi-idxgiadapter-getdesc).

## Cosa significa la percentuale GPU

La GPU contiene motori che possono lavorare contemporaneamente: 3D, compute,
copy, video e altri. RUST-CPU raggruppa i contatori dello stesso motore fisico,
somma i contributi dei PID su quel motore e limita la percentuale al 100%.
La percentuale dell'adattatore è il **massimo tra i motori**, seguendo la scelta
descritta da Microsoft per il Task Manager.

Per un PID viene mostrato il suo motore più occupato, considerando gli adattatori
DXGI riconosciuti. Non si sommano percentuali di motori differenti: due motori
occupati al 60% non diventano una GPU al 120%. Eventuali motori esposti nel gruppo
PDH ma privi di corrispondenza a un adattatore DXGI non vengono attribuiti come
GPU; questo evita anche di includere automaticamente dispositivi NPU.

Questa percentuale non esprime direttamente FLOPS, utilizzo di ogni unità di
calcolo, quota di potenza massima o velocità di inferenza. Anche un campione
valido pari a zero descrive solo il periodo osservato.

## Memoria dedicata, condivisa e processi

La memoria dedicata è gestita per uso GPU. Su una scheda discreta corrisponde
normalmente alla VRAM; su una GPU integrata può essere una parte della RAM
riservata dal firmware o dal driver. La memoria condivisa è RAM del sistema
usata dalla GPU. Le due grandezze vengono mantenute separate e non costituiscono
un unico quantitativo di VRAM disponibile.

| Campo JSON | Significato |
| --- | --- |
| `GpuRow.memory_total` | Capacità dedicata dichiarata da DXGI, o provider NVIDIA di fallback |
| `GpuRow.memory_used` | Dedicata usata, WDDM quando disponibile |
| `GpuRow.shared_memory_used` | RAM condivisa usata da quell'adattatore, WDDM |
| `ProcessRow.gpu_memory_bytes` | Dedicata attribuita al PID, totale sui suoi adattatori riconosciuti |
| `ProcessRow.gpu_shared_memory_bytes` | Condivisa attribuita al PID, tenuta distinta dalla dedicata |
| `ProcessRow.gpu_percent` | Percentuale del motore più occupato da quel PID |

Le risorse video condivise tra processi possono apparire nelle attribuzioni di
più PID. **Non sommare le righe dei processi per ricavare il consumo fisico totale**:
usare i contatori dell'adattatore. Inoltre l'attribuzione GPU al PID non prova
che tutta quella memoria appartenga a uno specifico modello LLM.

Microsoft documenta anche un problema noto dei contatori di memoria GPU per
processo su alcune versioni di Windows 10: possono mostrare valori crescenti
errati mentre la misura complessiva dell'adattatore resta corretta. Fonte:
[GPU Process Memory counters report incorrect values](https://learn.microsoft.com/en-us/troubleshoot/windows-client/performance/gpu-process-memory-counters-report-wrong-value).

## Sensori NVIDIA aggiuntivi

Quando `nvidia-smi` è già disponibile, un worker separato legge temperatura,
potenza e le metriche NVIDIA con un limite di attesa di 800 ms per comando e
intervallo normale di due secondi. Il processo è nascosto su Windows e non viene
invocato tramite una shell. Le letture più vecchie di sei secondi sono escluse.

Su Windows il carico e la memoria WDDM hanno precedenza quando validi; NVIDIA
integra temperatura e potenza e può fornire un fallback per metriche mancanti.
L'associazione tramite nome viene accettata soltanto se esatta e univoca: due
adattatori con lo stesso nome non ricevono arbitrariamente lo stesso sensore.
Le misure WDDM e NVIDIA possono differire per intervallo e criterio del driver.

Non viene usata `Win32_VideoController.AdapterRAM` come capacità moderna di
VRAM: quella proprietà WMI è limitata a 32 bit e non rappresenta correttamente
molti adattatori attuali.

## Temperature e tensioni opzionali con LibreHardwareMonitor

RUST-CPU continua a leggere i sensori termici esposti da sysinfo. Su Windows
può inoltre consumare in sola lettura il namespace WMI
`ROOT\LibreHardwareMonitor`, **se LibreHardwareMonitor è già in esecuzione** e
pubblica i sensori. L'applicazione non installa, avvia o configura quel programma,
un suo servizio o un driver.

La query seleziona `Name`, `SensorType`, `Value` e `Parent` dalla classe `Sensor`
per i tipi `Temperature` e `Voltage`. Il percorso hardware viene usato per
classificare il sensore; negli output compaiono nome leggibile, fonte e unità:

- Le temperature entrano in `Snapshot.sensors`, con prefisso `LHM` e gradi Celsius.
- Le tensioni entrano in `Snapshot.sensor_details`, con prefisso `LHM`, unità
  volt e indicazione di lettura live.

Il campionamento attivo avviene normalmente ogni due secondi; in assenza di un
provider utile il tentativo viene ripetuto ogni quindici secondi. Le misure
vecchie di quattro secondi vengono omesse. Un namespace assente, un sensore
senza valore o un numero non finito produce un'assenza dichiarata.

Il progetto LibreHardwareMonitor mostra direttamente il requisito di programma
in esecuzione e l'accesso al namespace nel suo
[esempio WMI ufficiale](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/blob/master/LibreHardwareMonitor.Windows.Forms/TestScripts/basicwmi.py).
La disponibilità di temperature e tensioni dipende da scheda madre, CPU, versione
del provider e permessi con cui è stato avviato; RUST-CPU non promette un sensore
che il provider non pubblica.

## CPU, memoria e limiti fondamentali

CPUID su x86/x86-64 riporta identificazione, istruzioni, capacità di indirizzamento
e descrittori cache. Le cache sono descritte per istanza riportata, senza sommare
automaticamente tutti i core; sistemi ibridi possono avere core con cache e
caratteristiche differenti. Una feature hardware riportata da CPUID non prova
che il sistema operativo o un runtime LLM la stia effettivamente usando.

Le frequenze sysinfo possono essere nominali. La media riportata non misura
ogni clock effettivo, né sostituisce una misura dei cicli durante il carico.
Il carico dei processi CPU viene normalizzato sulla capacità logica totale
dell'host: un thread completamente occupato su 16 CPU logiche vale circa 6,25%.

WMI fornisce inventario di scheda madre, BIOS e DIMM. Velocità configurata,
tensione configurata e capacità dichiarate dal firmware sono etichettate come
tali: non sono letture live dei segnali elettrici. L'inventario è acquisito una
volta con limite di attesa iniziale e non sostituisce il provider sensori live.

**SPD diretto, timing RAM reali, accesso SMBus/MSR e sensori proprietari non
pubblicati richiedono un provider specifico o accesso privilegiato.** Non vengono
simulati né installati driver per completarli. Questa versione non implementa
SMART o diagnostica fisica dei supporti di memorizzazione.

Su Linux restano disponibili sysinfo, CPUID sulle architetture supportate e il
provider NVIDIA quando presente. WDDM, DXGI, WMI di inventario e WMI LHM sono
provider Windows. Un risultato di compilazione su una piattaforma non certifica
automaticamente il funzionamento di un sensore sull'altra.
