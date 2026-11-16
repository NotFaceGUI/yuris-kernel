import AVFoundation
import UIKit

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    var window: UIWindow?

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        // cpal 的 iOS CoreAudio 后端只管 RemoteIO,不配 AVAudioSession。
        // 默认 soloAmbient 会被静音键静音;这里按媒体类应用设 playback。
        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.playback, mode: .default)
            try session.setActive(true)
        } catch {
            NSLog("AVAudioSession 配置失败: \(error)")
        }

        let window = UIWindow(frame: UIScreen.main.bounds)
        let navigation = UINavigationController(rootViewController: SetupViewController())
        navigation.navigationBar.prefersLargeTitles = false
        window.rootViewController = navigation
        window.makeKeyAndVisible()
        self.window = window
        return true
    }
}
