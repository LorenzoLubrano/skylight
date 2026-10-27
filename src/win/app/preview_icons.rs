//! Solo per le anteprime (`--snapshot`) fuori da Windows: i codici delle icone
//! di Segoe Fluent Icons / MDL2 Assets tradotti in quelli delle Fluent System
//! Icons (licenza MIT), che si possono installare anche su Linux.

pub fn map(c: char) -> char {
    let code = match c as u32 {
        0xE710 => 0xF10A, // add
        0xE706 => 0xE1F8, // brightness_high
        0xE711 => 0xF36A, // dismiss
        0xE73E => 0xF295, // checkmark
        0xE8FB => 0xF295, // checkmark
        0xE74D => 0xF34D, // delete
        0xE8A5 => 0xF379, // document
        0xEB42 => 0xE591, // drop
        0xE968 => 0xE99A, // plug_connected
        0xE8B7 => 0xF419, // folder
        0xE774 => 0xF45B, // globe
        0xE7F6 => 0xF472, // headphones
        0xE765 => 0xF4B9, // keyboard
        0xE720 => 0xE7FA, // mic
        0xE8D6 => 0xE855, // music_note_2
        0xEC4F => 0xE852, // music_note_1
        0xE74F => 0xEB4B, // speaker_mute
        0xE893 => 0xF56A, // next
        0xE892 => 0xF629, // previous
        0xF384 => 0xEE5A, // wifi_off
        0xE769 => 0xF5A2, // pause
        0xE768 => 0xF606, // play
        0xEB50 => 0xF115, // alert
        0xEA8F => 0xF115, // alert
        0xE72C => 0xF13E, // arrow_clockwise
        0xE713 => 0xF6AA, // settings
        0xE916 => 0xF827, // timer
        0xE7C3 => 0xF7CC, // text_description
        0xE7A7 => 0xF19A, // arrow_undo
        0xE992 => 0xF6F9, // speaker_0
        0xE993 => 0xF6FB, // speaker_1
        0xE994 => 0xEB43, // speaker_2
        0xE995 => 0xEB43, // speaker_2
        0xE767 => 0xEB43, // speaker_2
        0xE701 => 0xF8AD, // wifi_1
        0xE9D5 => 0xEC93, // task_list_ltr
        0xEC4A => 0xF831, // top_speed
        0xE76B => 0xF2AB, // chevron_left
        0xE76C => 0xF2B1, // chevron_right
        0xE787 => 0xE24F, // calendar_ltr
        0xE70F => 0xF3DE, // edit
        0xE702 => 0xF1DF, // bluetooth
        0xE8F2 => 0xF287, // chat
        0xE90A => 0xF300, // comment
        0xEB9D => 0xF3FF, // fast_forward
        0xEB9E => 0xF675, // rewind
        0xE738 => 0xEBD1, // subtract
        0xE740 => 0xE685, // full_screen_maximize
        0xE917 => 0xF2DE, // clock
        0xE8C8 => 0xF32C, // copy
        0xE8B1 => 0xEF37, // arrow_shuffle
        0xE83F => 0xE144, // battery_10
        0xE83E => 0xF1D0, // battery_charge
        0xEA93 => 0xF1D0, // battery_charge
        0xE850 => 0xF1BC, // battery_0
        0xE851 => 0xF1BE, // battery_1
        0xE852 => 0xF1C0, // battery_2
        0xE853 => 0xF1C2, // battery_3
        0xE854 => 0xF1C4, // battery_4
        0xE855 => 0xF1C6, // battery_5
        0xE856 => 0xF1C8, // battery_6
        0xE857 => 0xF1CA, // battery_7
        0xE858 => 0xF1CC, // battery_8
        0xE859 => 0xF1CE, // battery_9
        0xE85A => 0xF1D0, // battery_charge
        0xE85B => 0xF1D0, // battery_charge
        0xE85C => 0xF1D0, // battery_charge
        0xE85D => 0xF1D0, // battery_charge
        0xE85E => 0xF1D0, // battery_charge
        0xE85F => 0xF1D0, // battery_charge
        0xE860 => 0xF1D0, // battery_charge
        0xE861 => 0xF1D0, // battery_charge
        0xE862 => 0xF1D0, // battery_charge
        0xE708 => 0xF87E, // weather_moon
        0xE896 => 0xF151, // arrow_download
        _ => return c,
    };
    char::from_u32(code).unwrap_or(c)
}
