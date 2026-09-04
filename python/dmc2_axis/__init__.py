"""DMC2 extensions for LinuxCNC's AXIS user interface.

The package intentionally performs no eager imports.  AXIS loads each focused
extension through ``axis_user_command`` so an import failure is attributed to
that extension's typed operator-recovery boundary instead of preventing every
other independent control from loading.
"""
