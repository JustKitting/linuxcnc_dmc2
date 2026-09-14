# Runtime machine state

LinuxCNC writes `linuxcnc-position.txt` here when a session exits cleanly and
loads it at the next startup through `[TRAJ]POSITION_FILE`.  The generated
position file is deliberately ignored by Git because it describes the local
machine's runtime state, not source configuration.
