# Changelog

## 0.2.0 — 2026-10-19

- SQLite incorporato: archivio sessioni, campioni, benchmark, avvisi e preferenze.
- Riproduzione delle sessioni, nomi modificabili, eliminazione confermata, backup coerenti,
  import/export JSON e confronto A/B con medie, picchi, P95 e delta.
- Benchmark ripetibili e confronti separati per modello, metodo, algoritmo e build;
  avvio automatico di sessioni per le prove CPU/LLM.
- Impostazioni persistenti e avvisi configurabili per CPU, RAM, VRAM e temperature disponibili.
- GPU Windows WDDM/DXGI per AMD/Intel/NVIDIA, memoria dedicata/condivisa e attribuzione per PID.
- Letture sensori LibreHardwareMonitor quando il provider WMI è già disponibile.
- Proxy observer locale facoltativo per richieste LLM esterne instradate volontariamente,
  credenziali runtime da variabili d'ambiente e nessun prompt/risposta archiviato.
- Guida e navigazione corrette, tre nuove viste, rendering e test estesi.

## 0.1.0 — 2026-10-19

Prima release pubblica: TUI italiana con dieci viste, inventario hardware Windows,
CPUID, telemetria di sistema, NVIDIA opzionale, sonde LLM locali, benchmark esplicito,
Capacity Lab, snapshot JSON, sessioni CSV, CLI, installer per utente e versione portable.

Questa release privilegia letture documentate e assenze esplicite: nessun driver kernel,
nessuna lettura SPD raw, nessun controllo overclock, nessuna promessa di sensori universali.
