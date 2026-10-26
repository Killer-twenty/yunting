using System;
using System.Diagnostics;
using System.IO;
using System.Windows.Forms;

class Launcher {
  [STAThread]
  static void Main() {
    string dir = AppDomain.CurrentDomain.BaseDirectory;
    string unpacked = Path.Combine(dir, "win-unpacked");
    string exe = Path.Combine(unpacked, "云亭.exe");
    if (!File.Exists(exe)) {
      MessageBox.Show("未找到:\n" + exe, "云亭·启动器");
      return;
    }
    var psi = new ProcessStartInfo { FileName = exe, WorkingDirectory = unpacked, UseShellExecute = false };
    psi.EnvironmentVariables["ELECTRON_DISABLE_SANDBOX"] = "1";
    psi.EnvironmentVariables["ELECTRON_ENABLE_LOGGING"] = "1";
    try {
      var proc = Process.Start(psi);
      // 等 12 秒看它是否秒退——秒退时抓退出码弹窗给用户看
      if (proc.WaitForExit(12000)) {
        string env1 = ReadRecently("yunting");
        MessageBox.Show(
          "云亭主程序启动后立即退出了\n退出码: " + proc.ExitCode +
          "\n\n最后诊断日志:\n" + (env1 ?? "(无)"),
          "云亭·启动器", MessageBoxButtons.OK, MessageBoxIcon.Warning);
      }
    } catch (Exception ex) {
      MessageBox.Show("启动失败: " + ex.Message, "云亭·启动器", MessageBoxButtons.OK, MessageBoxIcon.Error);
    }
  }

  static string ReadRecently(string sub) {
    try {
      string log = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
        "yunting", "main-diag.log");
      if (File.Exists(log)) {
        var lines = File.ReadAllLines(log);
        int n = Math.Min(4, lines.Length);
        return string.Join("\n", lines, lines.Length - n, n);
      }
    } catch { }
    return null;
  }
}
