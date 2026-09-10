param([string]$Executable = 'target/release/deps/cpu8080_adaptive_classic_diagnostics-83b175c2e7d580c9.exe')
Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Linq;
using System.Runtime.InteropServices;
using System.Threading;
public static class CpuSample {
 [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
 [DllImport("kernel32.dll")] static extern uint SuspendThread(IntPtr thread);
 [DllImport("kernel32.dll")] static extern uint ResumeThread(IntPtr thread);
 [DllImport("kernel32.dll")] static extern bool GetThreadContext(IntPtr thread, IntPtr context);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
 [DllImport("dbghelp.dll", CharSet=CharSet.Ansi, SetLastError=true)] static extern bool SymInitialize(IntPtr process, string path, bool invade);
 [DllImport("dbghelp.dll")] static extern bool SymFromAddr(IntPtr process, ulong address, out ulong displacement, IntPtr symbol);
 [DllImport("dbghelp.dll")] static extern bool SymCleanup(IntPtr process);
 [DllImport("winmm.dll")] static extern uint timeBeginPeriod(uint period);
 [DllImport("winmm.dll")] static extern uint timeEndPeriod(uint period);
 public static void Run(string exe) {
  var counts = new Dictionary<string,int>(); int samples=0, failures=0;
  IntPtr allocation=Marshal.AllocHGlobal(1250);
  IntPtr context=new IntPtr((allocation.ToInt64()+15)&~15L);
  IntPtr symbol=Marshal.AllocHGlobal(1112);
  timeBeginPeriod(1);
  try {
   for(int round=0; round<5; round++) {
    var start=new ProcessStartInfo(exe, "full_system_runs_cputest_with_reference_totals --ignored --nocapture --test-threads=1");
    start.UseShellExecute=false; start.CreateNoWindow=true; start.RedirectStandardOutput=true; start.RedirectStandardError=true;
    using(var p=Process.Start(start)) {
     var output=p.StandardOutput.ReadToEndAsync(); var error=p.StandardError.ReadToEndAsync();
     Thread.Sleep(100);
     IntPtr processHandle;
     try { processHandle=p.Handle; } catch(InvalidOperationException) { p.WaitForExit(); Console.WriteLine("ROUND "+round+" exit="+p.ExitCode); Console.WriteLine(error.Result); if(p.ExitCode!=0) throw new Exception(output.Result); continue; }
     if(!SymInitialize(processHandle,System.IO.Path.GetDirectoryName(exe),true)) throw new Exception("SymInitialize: "+Marshal.GetLastWin32Error());
     IntPtr thread=IntPtr.Zero;
     try {
      int iteration=0;
      while(true) {
       try { if(p.HasExited) break; } catch(InvalidOperationException) { break; }
       if(iteration++%50==0) {
        if(thread!=IntPtr.Zero) { CloseHandle(thread); thread=IntPtr.Zero; }
        ProcessThread hottest=null;
        try {
         p.Refresh();
         foreach(ProcessThread candidate in p.Threads) {
          try { if(hottest==null || candidate.TotalProcessorTime>hottest.TotalProcessorTime) hottest=candidate; }
          catch(InvalidOperationException) {}
          catch(Win32Exception) {}
         }
        } catch(InvalidOperationException) { break; }
          catch(Win32Exception) { break; }
        if(hottest!=null) {
         try { thread=OpenThread(0x4a,false,(uint)hottest.Id); }
         catch(InvalidOperationException) { thread=IntPtr.Zero; }
        }
       }
       ulong ip=0;
       if(thread!=IntPtr.Zero) {
        uint suspended=SuspendThread(thread);
        if(suspended!=uint.MaxValue) {
         try {
          Marshal.WriteInt32(context,48,0x100001);
          if(GetThreadContext(thread,context)) ip=(ulong)Marshal.ReadInt64(context,248); else failures++;
         } finally { ResumeThread(thread); }
        } else failures++;
       } else failures++;
       if(ip!=0) {
        Marshal.WriteInt32(symbol,0,88); Marshal.WriteInt32(symbol,80,1024);
        ulong displacement;
        string name=SymFromAddr(processHandle,ip,out displacement,symbol) ? Marshal.PtrToStringAnsi(IntPtr.Add(symbol,84),Marshal.ReadInt32(symbol,76)) : "0x"+ip.ToString("x");
        if(!counts.ContainsKey(name)) counts[name]=0;
        counts[name]++; samples++;
       }
       Thread.Sleep(1);
      }
     } finally { if(thread!=IntPtr.Zero) CloseHandle(thread); SymCleanup(processHandle); }
     p.WaitForExit();
     Console.WriteLine("ROUND "+round+" exit="+p.ExitCode);
     Console.WriteLine(error.Result);
     if(p.ExitCode!=0) throw new Exception(output.Result);
    }
   }
  } finally { timeEndPeriod(1); Marshal.FreeHGlobal(allocation); Marshal.FreeHGlobal(symbol); }
  Console.WriteLine("SAMPLES="+samples+" FAILURES="+failures);
  foreach(var pair in counts.OrderByDescending(x=>x.Value).Take(35)) Console.WriteLine(pair.Value+" "+(100.0*pair.Value/Math.Max(1,samples)).ToString("F1")+"% "+pair.Key);
 }
}
"@
[CpuSample]::Run((Resolve-Path -LiteralPath $Executable).Path)
