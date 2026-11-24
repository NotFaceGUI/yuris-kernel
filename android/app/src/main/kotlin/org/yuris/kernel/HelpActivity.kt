package org.yuris.kernel

import android.os.Bundle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.appcompat.app.AppCompatActivity

/**
 * 参数说明静态页。
 *
 * <p>独立 Activity 而不是同页切换,目的是复用系统默认的进入/返回动画,
 * 与「游戏库 → 游戏详情」的转场保持一致。
 */
class HelpActivity : AppCompatActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            YuTheme {
                HelpScreen(onBack = { finish() })
            }
        }
    }
}
