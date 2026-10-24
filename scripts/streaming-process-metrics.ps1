$ErrorActionPreference = 'Stop'
# Persistent Win32 job accounting. No child is missed merely because it exits
# between RSS samples: job CPU and logical I/O remain cumulative.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
public static class StreamingProcessMetrics {
  [StructLayout(LayoutKind.Sequential)] struct BasicAccounting {
    public long User, Kernel, PeriodUser, PeriodKernel;
    public uint Faults, TotalProcesses, ActiveProcesses, TerminatedProcesses;
  }
  [StructLayout(LayoutKind.Sequential)] struct IoCounters {
    public ulong ReadOperations, WriteOperations, OtherOperations, ReadBytes, WriteBytes, OtherBytes;
  }
  [StructLayout(LayoutKind.Sequential)] struct Accounting { public BasicAccounting Basic; public IoCounters Io; }
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateJobObject(IntPtr a,string name);
  [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint access,bool inherit,int pid);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool AssignProcessToJobObject(IntPtr job,IntPtr process);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr job,int kind,out Accounting info,int size,IntPtr length);
  [DllImport("kernel32.dll",SetLastError=true,EntryPoint="QueryInformationJobObject")] static extern bool QueryIds(IntPtr job,int kind,IntPtr info,int size,IntPtr length);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateJobObject(IntPtr job,uint exit);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessIoCounters(IntPtr process,out IoCounters counters);
  sealed class Job {
    public string Label; public int Parent; public IntPtr Handle;
    public long PeakRss, Samples, LastSampleTicks, MaxSampleGapTicks;
    public Job(string label,int pid) {
      Label=label; Parent=pid; Handle=CreateJobObject(IntPtr.Zero,null);
      if(Handle==IntPtr.Zero)throw new Win32Exception();
      var process=OpenProcess(0x501,false,pid);
      if(process==IntPtr.Zero)throw new Win32Exception();
      try { if(!AssignProcessToJobObject(Handle,process))throw new Win32Exception(); }
      finally {CloseHandle(process);}
    }
    public string Snapshot() {
      Accounting a;
      if(!QueryInformationJobObject(Handle,8,out a,Marshal.SizeOf(typeof(Accounting)),IntPtr.Zero))throw new Win32Exception();
      var ids=new List<int>(); long rss=0;
      const int capacity=1024; int size=8+capacity*IntPtr.Size; var buffer=Marshal.AllocHGlobal(size);
      try {
        if(!QueryIds(Handle,3,buffer,size,IntPtr.Zero))throw new Win32Exception();
        int count=Marshal.ReadInt32(buffer,4);
        for(int i=0;i<count;i++) {
          int pid=IntPtr.Size==8?(int)Marshal.ReadInt64(buffer,8+i*IntPtr.Size):Marshal.ReadInt32(buffer,8+i*IntPtr.Size);
          ids.Add(pid);
          try {using(var p=Process.GetProcessById(pid))rss+=p.WorkingSet64;}catch(ArgumentException){}catch(InvalidOperationException){}
        }
      } finally {Marshal.FreeHGlobal(buffer);}
      long parentUser=0,parentKernel=0,parentRss=0;IoCounters parentIo=new IoCounters();
      try {using(var parent=Process.GetProcessById(Parent)){parentUser=parent.UserProcessorTime.Ticks;parentKernel=parent.PrivilegedProcessorTime.Ticks;parentRss=parent.WorkingSet64;GetProcessIoCounters(parent.Handle,out parentIo);}}catch(ArgumentException){}catch(InvalidOperationException){}
      long now=Stopwatch.GetTimestamp(); if(LastSampleTicks>0)MaxSampleGapTicks=Math.Max(MaxSampleGapTicks,now-LastSampleTicks); LastSampleTicks=now;
      PeakRss=Math.Max(PeakRss,rss); Samples++;
      return "{\"label\":\""+Label+"\",\"parentPid\":"+Parent+",\"activePids\":["+String.Join(",",ids)+"],\"processesCreated\":"+a.Basic.TotalProcesses+",\"activeProcesses\":"+a.Basic.ActiveProcesses+",\"cpuUser100ns\":"+a.Basic.User+",\"cpuKernel100ns\":"+a.Basic.Kernel+",\"parentCpuUser100ns\":"+parentUser+",\"parentCpuKernel100ns\":"+parentKernel+",\"parentRssBytes\":"+parentRss+",\"parentReadTransferBytes\":"+parentIo.ReadBytes+",\"parentWriteTransferBytes\":"+parentIo.WriteBytes+",\"readTransferBytes\":"+a.Io.ReadBytes+",\"writeTransferBytes\":"+a.Io.WriteBytes+",\"otherTransferBytes\":"+a.Io.OtherBytes+",\"rssBytes\":"+rss+",\"peakObservedRssBytes\":"+PeakRss+",\"rssSamples\":"+Samples+",\"maxRssSampleGapMs\":"+((long)(MaxSampleGapTicks*1000.0/Stopwatch.Frequency))+"}";
    }
  }
  public static void Run() {
    var jobs=new List<Job>(); Console.WriteLine("{\"ready\":true,\"method\":\"Win32 Job Object CPU and logical IO; sampled RSS\"}");
    try {
      while(true) {
        var input=Task.Run(()=>Console.ReadLine());
        while(!input.Wait(100))foreach(var job in jobs)job.Snapshot();
        var line=input.Result; if(line==null||line=="stop")break;
        try {
          var parts=line.Split(' ');
          if(parts[0]=="attach") {
            if(parts.Length!=3||!System.Text.RegularExpressions.Regex.IsMatch(parts[1],"^[a-zA-Z0-9_-]+$"))throw new Exception("invalid attach");
            var job=new Job(parts[1],Int32.Parse(parts[2]));jobs.Add(job);Console.WriteLine("{\"attached\":\""+parts[1]+"\"}");
          } else if(parts[0]=="reset") {foreach(var job in jobs){job.PeakRss=0;job.Samples=0;job.LastSampleTicks=0;job.MaxSampleGapTicks=0;}Console.WriteLine("{\"reset\":true}");
          } else if(parts[0]=="sample") {
            var rows=new List<string>();foreach(var job in jobs)rows.Add(job.Snapshot());Console.WriteLine("{\"jobs\":["+String.Join(",",rows)+"]}");
          } else throw new Exception("unknown command");
        } catch(Exception e) {Console.WriteLine("{\"error\":\""+e.Message.Replace("\\","\\\\").Replace("\"","\\\"").Replace("\r"," ").Replace("\n"," ")+"\"}");}
      }
    } finally {foreach(var job in jobs){TerminateJobObject(job.Handle,0);CloseHandle(job.Handle);}}
  }
}
'@
[StreamingProcessMetrics]::Run()
