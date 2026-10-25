import ServiceManagement

/// Login item for the main app. `.requiresApproval` means the user already
/// asked to launch at login; System Settings still has to confirm it.
enum LoginItem {
    static var status: SMAppService.Status {
        SMAppService.mainApp.status
    }

    static var isEnabled: Bool {
        switch status {
        case .enabled, .requiresApproval:
            true
        case .notRegistered, .notFound:
            false
        @unknown default:
            false
        }
    }

    static func setEnabled(_ enabled: Bool) throws {
        if enabled {
            try SMAppService.mainApp.register()
        } else {
            try SMAppService.mainApp.unregister()
        }
    }

    static func openSystemSettingsIfApprovalRequired() {
        guard status == .requiresApproval else { return }
        SMAppService.openSystemSettingsLoginItems()
    }
}
