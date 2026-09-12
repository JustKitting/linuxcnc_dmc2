# AXIS places notifications over sibling widgets. On this Tk/X11 display,
# moving that overlay during a layout change can retain blank sibling pixels.
# Repaint the overlay's children after layout/visibility changes have settled.
# This file handles Tk presentation only: no HAL or LinuxCNC commands.
namespace eval ::dmc2::notification_paint {
    variable targets
    variable pending
    variable last_error
}

proc ::dmc2::notification_paint::install {window} {
    variable targets
    set tag Dmc2NotificationPaint
    foreach source [list $window [winfo toplevel $window]] {
        set targets($source) $window
        set tags [bindtags $source]
        if {$tag ni $tags} {
            bindtags $source [concat $tags [list $tag]]
        }
    }
    foreach event {<Configure> <Map> <Visibility>} {
        bind $tag $event {::dmc2::notification_paint::schedule %W}
    }
    bind $tag <Destroy> {::dmc2::notification_paint::forget %W}
    schedule $window
}

proc ::dmc2::notification_paint::schedule {source} {
    variable targets
    variable pending
    if {![info exists targets($source)]} {return}
    set window $targets($source)
    if {![info exists pending($window)] && [winfo exists $window]} {
        set pending($window) [after idle [list ::dmc2::notification_paint::redraw $window]]
    }
}

proc ::dmc2::notification_paint::redraw {window} {
    variable pending
    variable last_error
    unset -nocomplain pending($window)
    if {![winfo exists $window] || ![winfo ismapped $window]} {return}
    if {[catch {
        set queue [list $window]
        while {[llength $queue]} {
            set child [lindex $queue 0]
            set queue [concat [lrange $queue 1 end] [winfo children $child]]
            if {[winfo ismapped $child]} {
                event generate $child <Expose> -when tail
            }
        }
    } cause]} {
        if {![info exists last_error($window)] || $last_error($window) ne $cause} {
            set last_error($window) $cause
            tk_messageBox -parent [winfo toplevel $window] -icon error -type ok \
                -title "Notification redraw failed" \
                -message "The notification popup could not be repainted: $cause\n\nClose this message and resize the window to retry. The toolbar Clear Fault and Pendant Mode controls remain independent."
        }
    } else {
        unset -nocomplain last_error($window)
    }
}

proc ::dmc2::notification_paint::forget {source} {
    variable targets
    variable pending
    variable last_error
    unset -nocomplain targets($source) last_error($source)
    if {[info exists pending($source)]} {
        after cancel $pending($source)
        unset pending($source)
    }
}
