import HerdbotKit
import ServiceManagement

@MainActor
func makeLoginItemControl() -> LoginItemControl {
    LoginItemControl(
        status: {
            switch SMAppService.mainApp.status {
            case .enabled: .enabled
            case .notRegistered: .notRegistered
            case .requiresApproval: .requiresApproval
            default: .unavailable
            }
        },
        register: { try SMAppService.mainApp.register() })
}
