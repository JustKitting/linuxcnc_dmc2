# Mesa 7I95T isolated-input investigation

Date: 2026-08-16

## Confirmed observations

- Board: Mesa 7I95T at `192.168.1.121`, MAC `00:60:1b:15:82:53`.
- Physical sensors are ROURCK SN04-N NPN normally-open proximity sensors.
- User visually observed the sensor LEDs and the Mesa input LEDs at the TB6 positions labelled IN10/IN11 activate; the PCB LED designators are CR19 and CR23. The connector labels and pin positions establish which inputs they indicate.
- Physical activation is accepted as authoritative evidence that input current reached those Mesa channels.
- HostMot2 exported `scan_width=24` and pins `inmux.00.input-00` through `input-23`, including raw and inverted variants.
- A timestamped 1 kHz trace independently recorded a synthetic transition, but INPUT9/10/11 remained zero.
- Therefore the recorder works, but the selected HostMot2 input data did not reflect the physical LEDs.

## Investigation loops

Results are recorded here before any further physical trigger is requested.

1. **Physical terminal identity.** Re-read the Mesa 7I95T manual connector table. TB6 pin 14 is INPUT9, pin 16 is INPUT10, and pin 17 is INPUT11. This confirms that the printed terminal numbers and HostMot2 input numbers are intended to match directly.
2. **Input-common grouping.** Re-read the same table. INPUT8/9 share TB6 pin 15; INPUT10/11 share TB6 pin 18. This rules out a software renumbering caused by the paired commons.
3. **LED meaning.** The observed LEDs are identified by both their TB6 input positions/labels (IN10/IN11) and PCB component designators (CR19/CR23). The terminal label and pin position are the functional identification; treating the CR number alone as contradictory was an error.
4. **Electrical path represented by the input LEDs.** Re-read manual pages 20-21. Each isolated input has a dedicated yellow status LED. The observed sensor LED plus the corresponding TB6 input LED proves the sensor switched and input current reached the board channel.
5. **NPN polarity.** Re-read manual page 21. For NPN/common-ground sensors, the paired INCOM connects to +5 to +36 V and the input is grounded to activate. This matches the sensor type and explains why an active LED is a real board-side event.
6. **Voltage threshold.** Re-read manual pages 21 and 46. The isolated input operating range is ±4 V to ±36 V, with approximately 4.7 kOhm series resistance. Five volts is within the documented range. A lit input LED eliminates low supply voltage as the explanation for HAL being permanently zero.
7. **Input bandwidth.** Re-read manual pages 21 and 46. The hardware input bandwidth is about 4-5 kHz. The human-operated proximity event and visible LED duration are far longer than the hardware minimum and should be observable.
8. **Correct HAL module.** Checked the official LinuxCNC HostMot2 manual. The 7I95-family isolated inputs are represented by the `inmux` module, with `input-NN` and `raw-input-NN` HAL pins. Monitoring ordinary GPIO instead would be wrong; the recorder used InMux.
9. **Correct channel span.** The live board's InMux control register was previously decoded by the driver as `scan_width=24`. Official 2.9.8 source computes this from hardware MaxBit + 1. Thus channels 0 through 23, including 9-11, exist in this loaded firmware.
10. **Exact installed release source.** Downloaded official LinuxCNC tag `v2.9.8`, commit `39bfc1874ac41eb9dd3088fe802dd1726fb68652`, matching the installed `linuxcnc-uspace 1:2.9.8` package.
11. **Driver-to-register mapping.** Audited `src/hal/drivers/mesa-hostmot2/inmux.c`. It reads filtered data at InMux base + 0x200 and raw data at base + 0x300, then maps bit `j` directly to HAL `input-j` and `raw-input-j`. There is no hidden offset or permutation in the LinuxCNC driver.
12. **Board register addresses.** The board's previously reported module descriptor gives InMux base 0x8000 and register stride 0x100. Therefore the exact FPGA registers are control 0x8000, filter 0x8100, filtered data 0x8200, raw data 0x8300, and MPG data 0x8400. These agree with the official 2.9.8 driver source.
13. **Filtered-versus-raw distinction.** Official source independently shows both registers are fetched every HostMot2 read. A debounce setting can delay `input-NN`, but cannot make `raw-input-NN` stay zero during a sustained physical activation if the FPGA raw register changes.
14. **Default debounce timing.** Official docs/source set scan rate 20 kHz and fast filtering to 5 scans, or 250 microseconds. The recorder's 1 ms servo samples are slower, but its latch/continuous trace should see a human-duration event. More importantly, the raw pin bypasses this filter.
15. **Required write function.** Official source shows `hm2_inmux_prepare_tram_write()` and `hm2_inmux_write()` program scan/filter control through the board write function. Earlier tests which omitted `hm2_7i95.0.write` were invalid. The later recorder includes it.
16. **Required read function.** Official source shows `hm2_inmux_process_tram_read()` is called by `hm2_7i95.0.read` only after a successful Ethernet transaction. The recorder schedules that function every 1 ms.
17. **Function ordering.** The recorder schedules board read, event/sample logic, then board write. That ordering makes the sample consume the newly published read values; the write configures the next cycle. Aside from the first startup cycle, this is valid.
18. **Recorder end-to-end test.** The timestamped sampler captured the deliberately generated SELF_TEST transition and stored both edges. This proves the realtime thread, sampler, userspace drain, and trace-file path were operating during the test.
19. **Latch component test.** A separate `flipflop` synthetic set/reset test succeeded. The failure to see IN9-11 was not caused by requiring the sensor to remain active until shell polling happened.
20. **Stale-process check.** Process inspection found no surviving LinuxCNC, halrun, recorder, or monitor process; only the inspection command itself matched. An old HAL owner is not currently holding the board or supplying stale pins.
21. **Installed module capability.** Inspected `/usr/lib/linuxcnc/modules/hostmot2.so`. Its strings contain the complete InMux pin names, filtered/raw paths, scan settings, and error messages expected from the audited source. This is not an old driver with no InMux support.
22. **Newer stable-driver comparison.** Compared official LinuxCNC v2.9.8 to v2.9.10 for both `inmux.c` and `hm2_eth.c`; there is no diff. Updating from 2.9.8 to 2.9.10 would not change this input path and is not an evidence-based fix.
23. **Firmware type boundary.** The Mesa manual explicitly warns never to load a 7I95 bitfile onto a 7I95T. The live board identifies as 7I95T and exposes a coherent 24-bit InMux, but the exact flashed bitfile identity still needs direct verification; this remains a live root-cause branch.
24. **Direct FPGA-register test design.** Derived a read-only, driver-independent test: read 0x8200 and 0x8300 over documented LBP16 UDP while observing the board LED. If bit 10/11 changes there, the fault is LinuxCNC scheduling/communications; if it does not, the fault is FPGA firmware/mux mapping downstream of the opto input.
25. **Direct-register access attempt.** Attempted both documented raw UDP and `mesaflash --rpo` reads. This session's execution sandbox rejected the UDP send with `Operation not permitted`, including after user approval. No register result was obtained, and none is claimed.
26. **Functional labels take precedence.** The four green HostMot2/debug LEDs are CR24-CR27. The LEDs the user identified are at the TB6 IN10/IN11 positions. Their CR component designators do not override the connector-position labels. IN10/IN11 activation is therefore confirmed visually.
27. **Network transport semantics.** Re-read manual pages 22 and 42-43. Board communication uses UDP/LBP16, and the board exposes parse, memory, write, bad-packet, and HostMot2-timeout counters. These counters and `hm2_7i95.0.packet-error`/`io_error` must be captured in the corrected diagnostic, not hidden by a coprocess pipe.
28. **Current certainty boundary.** Evidence proves the sensor event reaches the isolated input indicator and proves the previous trace machinery ran. It does not yet prove whether FPGA register 0x8300 changed. Claiming a wiring failure, dead sensor, or successful software detection at this point would be false.

## Narrowed fault boundary

The unresolved boundary is downstream of the confirmed TB6 input indication: between the input optocoupler/status circuit and the FPGA InMux raw-data register, or between that register and the HostMot2 Ethernet read transaction. The next valid test records all 24 raw/filtered bits plus packet/error state in one continuous timestamped capture.
