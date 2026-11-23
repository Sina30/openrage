# Keep agent work from degrading the desktop it runs on.
#
# A watcher that recognises processes agents launch and puts each one in a
# Job Object that forces Idle CPU priority and caps memory, and lowers its I/O
# and page priority. Children join the job at creation, so catching the root of
# an agent's process tree catches the whole tree with no polling race.
#
# A process is agent work when any of these holds, checked in this order:
#   - it is already in one of the watcher's jobs (inherited from its parent);
#   - its parent is agent work;
#   - its image path is under a root (the lane root, plus -ExtraRoots);
#   - its working directory is under a root;
#   - its environment carries a marker (CURSOR_AGENT=1, CLAUDECODE=1), which
#     every process an agent's shell starts inherits.
# The owner's own terminal carries no marker, so his `cargo` in the checkout is
# left alone; the one trade is that anything he starts *inside a lane
# directory* is treated as agent work.
#
# Memory limits are on commit, not working set, and are enforced by the
# kernel: an allocation past the ceiling fails. A process that survives its
# failed allocation is terminated by the watcher with exit code 137, so the
# runaway dies rather than the desktop.
#
# `JOB_OBJECT_LIMIT_PRIORITY_CLASS` needs SeIncreaseBasePriorityPrivilege,
# which a non-elevated user does not hold. Idle is applied per process instead
# and is inherited by every child at creation, which is the same effect for
# cargo and rustc; what it loses is that a member could raise itself again.

[CmdletBinding(DefaultParameterSetName = 'Status')]
param(
    [Parameter(ParameterSetName = 'Run')][switch]$Run,
    [Parameter(ParameterSetName = 'DryRun')][switch]$DryRun,
    [Parameter(ParameterSetName = 'Status')][switch]$Status,
    [Parameter(ParameterSetName = 'Stop')][switch]$Stop,
    [Parameter(ParameterSetName = 'Start')][switch]$Start,
    [Parameter(ParameterSetName = 'Install')][switch]$Install,
    [Parameter(ParameterSetName = 'Uninstall')][switch]$Uninstall,
    [Parameter(ParameterSetName = 'SelfTest')][switch]$SelfTest,
    [Parameter(ParameterSetName = 'Exec')][switch]$Exec,
    [Parameter(ParameterSetName = 'Cap')][int]$SetCap = -1,

    [string]$LaneRoot = "",
    [string[]]$ExtraRoots = @(),
    [string[]]$Markers = @('CURSOR_AGENT=1', 'CLAUDECODE=1'),
    [double]$ProcessMemoryGiB = 0,
    [double]$JobMemoryGiB = 0,
    [int]$CpuCapPercent = 0,
    [ValidateSet('Idle', 'BelowNormal', 'Normal')][string]$Priority = 'Idle',
    [int]$IntervalMs = 1000,
    [string]$StateDir = "",
    [string]$ScratchRoot = "",

    [Parameter(ParameterSetName = 'Exec')][string]$Command = ""
)

$ErrorActionPreference = "Stop"

$TaskName = 'OpenRage agent limits'
$InstanceName = 'Local\OpenRage.AgentLimits.Instance'
$StopName = 'Local\OpenRage.AgentLimits.Stop'

$source = @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;

namespace OpenRageLimits
{
    [StructLayout(LayoutKind.Sequential)]
    public struct BasicLimit
    {
        public long PerProcessUserTimeLimit;
        public long PerJobUserTimeLimit;
        public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize;
        public UIntPtr MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass;
        public uint SchedulingClass;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct IoCounters { public ulong R, W, O, RB, WB, OB; }

    [StructLayout(LayoutKind.Sequential)]
    public struct ExtendedLimit
    {
        public BasicLimit Basic;
        public IoCounters Io;
        public UIntPtr ProcessMemoryLimit;
        public UIntPtr JobMemoryLimit;
        public UIntPtr PeakProcessMemoryUsed;
        public UIntPtr PeakJobMemoryUsed;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct PortInfo { public IntPtr Key; public IntPtr Port; }

    [StructLayout(LayoutKind.Sequential)]
    public struct CpuRate { public uint ControlFlags; public uint Value; }

    [StructLayout(LayoutKind.Sequential)]
    public struct Accounting
    {
        public long TotalUserTime, TotalKernelTime, ThisPeriodUser, ThisPeriodKernel;
        public uint PageFaults, TotalProcesses, ActiveProcesses, TotalTerminated;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct ProcessEntry
    {
        public uint dwSize; public uint cntUsage; public uint th32ProcessID; public IntPtr th32DefaultHeapID;
        public uint th32ModuleID; public uint cntThreads; public uint th32ParentProcessID; public int pcPriClassBase;
        public uint dwFlags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string szExeFile;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct MemCounters
    {
        public uint cb; public uint PageFaultCount;
        public UIntPtr PeakWorkingSetSize, WorkingSetSize, QuotaPeakPagedPoolUsage, QuotaPagedPoolUsage,
            QuotaPeakNonPagedPoolUsage, QuotaNonPagedPoolUsage, PagefileUsage, PeakPagefileUsage, PrivateUsage;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct Pbi
    {
        public IntPtr ExitStatus, PebBaseAddress, AffinityMask, BasePriority, UniqueProcessId, InheritedFromUniqueProcessId;
    }

    public static class Native
    {
        [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] public static extern IntPtr CreateJobObject(IntPtr sa, string name);
        [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] public static extern IntPtr OpenJobObject(uint access, bool inherit, string name);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern bool SetInformationJobObject(IntPtr job, int cls, IntPtr info, uint len);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern bool QueryInformationJobObject(IntPtr job, int cls, IntPtr info, uint len, out uint ret);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern bool AssignProcessToJobObject(IntPtr job, IntPtr proc);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern bool IsProcessInJob(IntPtr proc, IntPtr job, out bool result);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern bool CloseHandle(IntPtr h);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern IntPtr CreateIoCompletionPort(IntPtr file, IntPtr existing, UIntPtr key, uint threads);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern bool GetQueuedCompletionStatus(IntPtr port, out uint bytes, out UIntPtr key, out IntPtr overlapped, uint ms);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern bool SetPriorityClass(IntPtr h, uint cls);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern uint GetPriorityClass(IntPtr h);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern bool SetProcessInformation(IntPtr h, int cls, ref uint info, uint size);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern bool GetProcessInformation(IntPtr h, int cls, out uint info, uint size);
        [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] public static extern bool QueryFullProcessImageName(IntPtr h, uint flags, StringBuilder sb, ref uint size);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern bool ReadProcessMemory(IntPtr h, IntPtr addr, byte[] buf, IntPtr size, out IntPtr read);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern bool TerminateProcess(IntPtr h, uint code);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern bool GetExitCodeProcess(IntPtr h, out uint code);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
        [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] public static extern bool Process32FirstW(IntPtr snap, ref ProcessEntry e);
        [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] public static extern bool Process32NextW(IntPtr snap, ref ProcessEntry e);
        [DllImport("kernel32.dll", SetLastError = true)] public static extern bool GetPhysicallyInstalledSystemMemory(out ulong kb);
        [DllImport("psapi.dll", SetLastError = true)] public static extern bool GetProcessMemoryInfo(IntPtr h, out MemCounters c, uint cb);
        [DllImport("ntdll.dll")] public static extern int NtSetInformationProcess(IntPtr h, int cls, ref int info, int len);
        [DllImport("ntdll.dll")] public static extern int NtQueryInformationProcess(IntPtr h, int cls, ref int info, int len, out int ret);
        [DllImport("ntdll.dll")] public static extern int NtQueryInformationProcess(IntPtr h, int cls, ref Pbi info, int len, out int ret);
    }

    public sealed class Member
    {
        public int Pid; public string Name; public string Reason; public ulong Commit; public uint PriorityClass; public int IoPriority; public uint PagePriority;
    }

    public sealed class Governor : IDisposable
    {
        public const uint IDLE = 0x40, BELOW_NORMAL = 0x4000, NORMAL = 0x20;
        public const uint KilledExitCode = 137;
        const uint PROCESS_ALL_WE_NEED = 0x1 | 0x10 | 0x100 | 0x200 | 0x400 | 0x1000;
        const uint PROCESS_QUERY_LIMITED = 0x1000;
        // Cursor runs every agent command's shell in a job of its own, so there is one hierarchy per command.
        const int MaxJobs = 512;

        public string[] Roots = new string[0];
        public string[] Markers = new string[0];
        public string[] ExcludeImages = new string[0];
        public int[] ScopePids = new int[0];
        public string JobName;
        public uint PriorityClass = IDLE;
        public ulong ProcessMemoryLimit;
        public ulong JobMemoryLimit;
        public uint CpuRatePercent;
        public int IoPriority = 0;
        public uint PagePriority = 1;
        public bool DryRun;

        public readonly List<string> Events = new List<string>();
        readonly List<string> log = new List<string>();
        sealed class Slot { public IntPtr Handle; public string Name; public bool Rooted; public int Serial; }
        readonly List<Slot> slots = new List<Slot>();
        int serial;
        readonly Dictionary<int, bool> known = new Dictionary<int, bool>();
        readonly Dictionary<int, string> reasons = new Dictionary<int, string>();
        IntPtr port = IntPtr.Zero;
        readonly int self = Process.GetCurrentProcess().Id;
        public int Assigned, Killed;

        public static ulong InstalledBytes()
        {
            ulong kb;
            if (Native.GetPhysicallyInstalledSystemMemory(out kb)) return kb * 1024UL;
            return 0;
        }

        public string[] JobNames { get { return slots.ConvertAll(s => s.Name).ToArray(); } }

        public List<string> DrainLog() { var l = new List<string>(log); log.Clear(); return l; }

        void Note(string s) { log.Add(DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss") + "  " + s); }

        public void Open()
        {
            if (DryRun) return;
            port = Native.CreateIoCompletionPort(new IntPtr(-1), IntPtr.Zero, UIntPtr.Zero, 1);
            if (port == IntPtr.Zero) throw new InvalidOperationException("CreateIoCompletionPort failed: " + Marshal.GetLastWin32Error());
        }

        Slot AddJob(bool isRooted)
        {
            int s = ++serial;
            string name = JobName == null ? null : JobName + "." + s;
            IntPtr job = Native.CreateJobObject(IntPtr.Zero, name);
            if (job == IntPtr.Zero) throw new InvalidOperationException("CreateJobObject failed: " + Marshal.GetLastWin32Error());
            if (Marshal.GetLastWin32Error() == 183) Note("job " + name + " already existed; reusing it");
            ApplyLimits(job, true);
            var pi = new PortInfo { Key = new IntPtr(s), Port = port };
            if (!SetInfo(job, 7, pi)) Note("job " + name + ": completion port refused (" + Marshal.GetLastWin32Error() + "); ceilings still hold but a survivor is not terminated");
            var slot = new Slot { Handle = job, Name = name ?? "(unnamed)", Rooted = isRooted, Serial = s };
            slots.Add(slot);
            return slot;
        }

        void Drop(Slot s)
        {
            Native.CloseHandle(s.Handle);
            slots.Remove(s);
        }

        Slot BySerial(int s) { return slots.Find(x => x.Serial == s); }

        static bool SetInfo<T>(IntPtr job, int cls, T value) where T : struct
        {
            int size = Marshal.SizeOf(typeof(T));
            IntPtr p = Marshal.AllocHGlobal(size);
            try { Marshal.StructureToPtr(value, p, false); return Native.SetInformationJobObject(job, cls, p, (uint)size); }
            finally { Marshal.FreeHGlobal(p); }
        }

        static bool QueryInfo<T>(IntPtr job, int cls, out T value) where T : struct
        {
            int size = Marshal.SizeOf(typeof(T));
            IntPtr p = Marshal.AllocHGlobal(size);
            try
            {
                uint ret;
                bool ok = Native.QueryInformationJobObject(job, cls, p, (uint)size, out ret);
                value = ok ? (T)Marshal.PtrToStructure(p, typeof(T)) : default(T);
                return ok;
            }
            finally { Marshal.FreeHGlobal(p); }
        }

        void ApplyLimits(IntPtr job, bool on)
        {
            var x = new ExtendedLimit();
            uint flags = 0;
            if (on && ProcessMemoryLimit != 0) { flags |= 0x100; x.ProcessMemoryLimit = new UIntPtr(ProcessMemoryLimit); }
            if (on && JobMemoryLimit != 0) { flags |= 0x200; x.JobMemoryLimit = new UIntPtr(JobMemoryLimit); }
            x.Basic.LimitFlags = flags;
            if (!SetInfo(job, 9, x)) Note("memory limits refused: " + Marshal.GetLastWin32Error());
            var c = new CpuRate();
            if (on && CpuRatePercent > 0) { c.ControlFlags = 0x1 | 0x4; c.Value = CpuRatePercent * 100; }
            if (!SetInfo(job, 15, c) && on && CpuRatePercent > 0) Note("cpu cap refused: " + Marshal.GetLastWin32Error());
        }

        public void Reapply() { foreach (var s in slots) ApplyLimits(s.Handle, true); }

        public ExtendedLimit Limits(int i) { ExtendedLimit x; QueryInfo(slots[i].Handle, 9, out x); return x; }
        public Accounting Account(int i) { Accounting a; QueryInfo(slots[i].Handle, 1, out a); return a; }
        public int JobCount { get { return slots.Count; } }

        public static int[] JobPids(IntPtr job)
        {
            const int cap = 8192;
            int size = 8 + cap * IntPtr.Size;
            IntPtr p = Marshal.AllocHGlobal(size);
            try
            {
                uint ret;
                if (!Native.QueryInformationJobObject(job, 3, p, (uint)size, out ret)) return new int[0];
                int n = Marshal.ReadInt32(p, 4);
                var r = new int[n];
                for (int k = 0; k < n; k++) r[k] = (int)Marshal.ReadIntPtr(p, 8 + k * IntPtr.Size).ToInt64();
                return r;
            }
            finally { Marshal.FreeHGlobal(p); }
        }

        public int[] Pids(int i) { return JobPids(slots[i].Handle); }

        public bool InJob(int pid)
        {
            IntPtr h = Native.OpenProcess(PROCESS_QUERY_LIMITED, false, pid);
            if (h == IntPtr.Zero) return false;
            try { return InAnyJob(h); } finally { Native.CloseHandle(h); }
        }

        bool InAnyJob(IntPtr h)
        {
            foreach (var s in slots) { bool r; if (Native.IsProcessInJob(h, s.Handle, out r) && r) return true; }
            return false;
        }

        // --- per-process inspection ---------------------------------------------------------

        public static string ImagePath(IntPtr h)
        {
            var sb = new StringBuilder(1024); uint n = 1024;
            return Native.QueryFullProcessImageName(h, 0, sb, ref n) ? sb.ToString() : null;
        }

        static IntPtr ReadPtr(IntPtr h, IntPtr at)
        {
            var b = new byte[8]; IntPtr got;
            if (!Native.ReadProcessMemory(h, at, b, new IntPtr(8), out got)) return IntPtr.Zero;
            return new IntPtr(BitConverter.ToInt64(b, 0));
        }

        static string ReadUnicodeString(IntPtr h, IntPtr at)
        {
            var hdr = new byte[16]; IntPtr got;
            if (!Native.ReadProcessMemory(h, at, hdr, new IntPtr(16), out got)) return null;
            int len = BitConverter.ToUInt16(hdr, 0);
            IntPtr buf = new IntPtr(BitConverter.ToInt64(hdr, 8));
            if (len == 0 || buf == IntPtr.Zero) return "";
            var b = new byte[len];
            if (!Native.ReadProcessMemory(h, buf, b, new IntPtr(len), out got)) return null;
            return Encoding.Unicode.GetString(b);
        }

        // x64 RTL_USER_PROCESS_PARAMETERS: CurrentDirectory.DosPath 0x38, Environment 0x80, EnvironmentSize 0x3F0.
        public static bool ReadPeb(IntPtr h, out string cwd, out string env)
        {
            cwd = null; env = null;
            var pbi = new Pbi(); int ret;
            if (Native.NtQueryInformationProcess(h, 0, ref pbi, Marshal.SizeOf(typeof(Pbi)), out ret) != 0) return false;
            if (pbi.PebBaseAddress == IntPtr.Zero) return false;
            IntPtr pp = ReadPtr(h, new IntPtr(pbi.PebBaseAddress.ToInt64() + 0x20));
            if (pp == IntPtr.Zero) return false;
            cwd = ReadUnicodeString(h, new IntPtr(pp.ToInt64() + 0x38));
            IntPtr envPtr = ReadPtr(h, new IntPtr(pp.ToInt64() + 0x80));
            long envSize = ReadPtr(h, new IntPtr(pp.ToInt64() + 0x3F0)).ToInt64();
            if (envPtr == IntPtr.Zero) return cwd != null;
            if (envSize <= 0 || envSize > (1 << 20)) envSize = 1 << 16;
            var b = new byte[envSize]; IntPtr got;
            while (envSize >= 4096 && !Native.ReadProcessMemory(h, envPtr, b, new IntPtr(envSize), out got))
            {
                envSize /= 2; b = new byte[envSize];
            }
            if (envSize < 4096 && !Native.ReadProcessMemory(h, envPtr, b, new IntPtr(envSize), out got)) return cwd != null;
            env = Encoding.Unicode.GetString(b);
            return true;
        }

        public static bool HasMarker(string env, string marker)
        {
            if (env == null) return false;
            int eq = marker.IndexOf('=');
            string name = eq < 0 ? marker : marker.Substring(0, eq);
            string value = eq < 0 ? null : marker.Substring(eq + 1);
            foreach (var entry in env.Split('\0'))
            {
                if (entry.Length == 0) continue;
                int e = entry.IndexOf('=', 1);
                if (e < 0) continue;
                if (!string.Equals(entry.Substring(0, e), name, StringComparison.OrdinalIgnoreCase)) continue;
                string v = entry.Substring(e + 1);
                if (value == null ? v.Length > 0 : v == value) return true;
            }
            return false;
        }

        string UnderRoot(string path)
        {
            if (string.IsNullOrEmpty(path)) return null;
            string p = path.TrimEnd('\\') + "\\";
            foreach (var r in Roots)
                if (p.StartsWith(r, StringComparison.OrdinalIgnoreCase)) return r;
            return null;
        }

        public static int GetIoPriority(IntPtr h) { int v = -1, ret; Native.NtQueryInformationProcess(h, 33, ref v, 4, out ret); return v; }
        public static uint GetPagePriority(IntPtr h) { uint v; return Native.GetProcessInformation(h, 0, out v, 4) ? v : 0; }
        public static ulong Commit(IntPtr h)
        {
            MemCounters c;
            return Native.GetProcessMemoryInfo(h, out c, (uint)Marshal.SizeOf(typeof(MemCounters))) ? c.PrivateUsage.ToUInt64() : 0;
        }

        void Lower(IntPtr h)
        {
            if (PriorityClass != 0) Native.SetPriorityClass(h, PriorityClass);
            int io = IoPriority; Native.NtSetInformationProcess(h, 33, ref io, 4);
            uint pg = PagePriority; Native.SetProcessInformation(h, 0, ref pg, 4);
        }

        static void Restore(IntPtr h)
        {
            Native.SetPriorityClass(h, NORMAL);
            int io = 2; Native.NtSetInformationProcess(h, 33, ref io, 4);
            uint pg = 5; Native.SetProcessInformation(h, 0, ref pg, 4);
        }

        // --- the loop -----------------------------------------------------------------------

        public void Tick(int timeoutMs)
        {
            if (!DryRun && port != IntPtr.Zero)
            {
                var until = DateTime.UtcNow.AddMilliseconds(timeoutMs);
                while (true)
                {
                    int left = (int)(until - DateTime.UtcNow).TotalMilliseconds;
                    if (left < 0) left = 0;
                    uint msg; UIntPtr key; IntPtr ov;
                    if (!Native.GetQueuedCompletionStatus(port, out msg, out key, out ov, (uint)left)) break;
                    Handle(msg, BySerial((int)key.ToUInt64()), (int)ov.ToInt64());
                    if (left == 0) break;
                }
            }
            else if (timeoutMs > 0) System.Threading.Thread.Sleep(timeoutMs);
            Scan();
            if (!DryRun) EnforceAggregate();
        }

        // Each command's tree has a job of its own, so no kernel limit spans all agent work; the sum is polled.
        void EnforceAggregate()
        {
            if (JobMemoryLimit == 0 || slots.Count == 0) return;
            var seen = new HashSet<int>();
            ulong sum = 0, most = 0; int victim = -1;
            foreach (var s in slots)
                foreach (int m in JobPids(s.Handle))
                {
                    if (!seen.Add(m)) continue;
                    ulong c = 0;
                    WithProcess(m, PROCESS_QUERY_LIMITED, h => { c = Commit(h); });
                    sum += c;
                    if (c > most) { most = c; victim = m; }
                }
            if (sum <= JobMemoryLimit || victim < 0) return;
            Events.Add("aggregate:" + victim);
            Kill(victim, "was the largest agent process (" + Mb(most) + " of " + Mb(sum) + ") when all agent work passed its ceiling of " + Mb(JobMemoryLimit));
        }

        void Handle(uint msg, Slot slot, int pid)
        {
            switch (msg)
            {
                case 4:
                    Accounting a;
                    if (slot != null && !slot.Rooted && QueryInfo(slot.Handle, 1, out a) && a.ActiveProcesses == 0) Drop(slot);
                    break;
                case 6:
                    if (!known.ContainsKey(pid)) { known[pid] = true; reasons[pid] = "inherited"; }
                    WithProcess(pid, PROCESS_ALL_WE_NEED, Lower);
                    break;
                case 9:
                    Events.Add("process-limit:" + pid);
                    Kill(pid, "passed the per-process ceiling of " + Mb(ProcessMemoryLimit));
                    break;
                case 10:
                    Events.Add("job-limit:" + pid);
                    int victim = -1; ulong most = 0, sum = 0;
                    if (slot != null)
                        foreach (int m in JobPids(slot.Handle))
                        {
                            ulong c = 0;
                            WithProcess(m, PROCESS_QUERY_LIMITED, h => { c = Commit(h); });
                            sum += c;
                            if (c > most) { most = c; victim = m; }
                        }
                    // One burst of pressure refuses many allocations and posts a message for each. Kill only
                    // while the members really are near the ceiling, or one runaway costs every process after it.
                    ulong slack = Math.Max(JobMemoryLimit / 20, 64UL << 20);
                    if (victim < 0 || sum + slack < JobMemoryLimit)
                    {
                        Note("job ceiling refused an allocation by pid " + pid + " with members at " + Mb(sum) + " of " + Mb(JobMemoryLimit) + "; nobody killed");
                        break;
                    }
                    Kill(victim, "was the largest member (" + Mb(most) + " of " + Mb(sum) + ") when the job reached its ceiling of " + Mb(JobMemoryLimit) + "; pid " + pid + " made the allocation");
                    break;
            }
        }

        static string Mb(ulong b) { return (b / (1024 * 1024)) + " MiB"; }

        static void WithProcess(int pid, uint access, Action<IntPtr> f)
        {
            IntPtr h = Native.OpenProcess(access, false, pid);
            if (h == IntPtr.Zero) return;
            try { f(h); } finally { Native.CloseHandle(h); }
        }

        void Kill(int pid, string why)
        {
            IntPtr h = Native.OpenProcess(0x1 | 0x1000, false, pid);
            if (h == IntPtr.Zero) { Note("could not open pid " + pid + " to kill it (" + Marshal.GetLastWin32Error() + "); it " + why); return; }
            try
            {
                string img = ImagePath(h) ?? "?";
                ulong c = Commit(h);
                if (Native.TerminateProcess(h, KilledExitCode))
                {
                    Killed++;
                    Events.Add("killed:" + pid);
                    Note("KILLED pid " + pid + " " + img + " at " + Mb(c) + " commit: it " + why);
                }
                else Note("could not kill pid " + pid + " (" + Marshal.GetLastWin32Error() + "); it " + why);
            }
            finally { Native.CloseHandle(h); }
        }

        public sealed class Seen { public int Pid, Parent; public string Exe; }

        public static List<Seen> Snapshot()
        {
            var list = new List<Seen>();
            IntPtr snap = Native.CreateToolhelp32Snapshot(0x2, 0);
            if (snap == new IntPtr(-1)) return list;
            try
            {
                var e = new ProcessEntry(); e.dwSize = (uint)Marshal.SizeOf(typeof(ProcessEntry));
                if (!Native.Process32FirstW(snap, ref e)) return list;
                do { list.Add(new Seen { Pid = (int)e.th32ProcessID, Parent = (int)e.th32ParentProcessID, Exe = e.szExeFile }); }
                while (Native.Process32NextW(snap, ref e));
            }
            finally { Native.CloseHandle(snap); }
            return list;
        }

        bool InScope(int pid, Dictionary<int, int> parent)
        {
            if (ScopePids.Length == 0) return true;
            for (int i = 0; i < 64 && pid > 4; i++)
            {
                if (Array.IndexOf(ScopePids, pid) >= 0) return true;
                int p;
                if (!parent.TryGetValue(pid, out p) || p == pid) return false;
                pid = p;
            }
            return false;
        }

        public List<Member> Scan()
        {
            var found = new List<Member>();
            var snap = Snapshot();
            var parent = new Dictionary<int, int>();
            foreach (var s in snap) parent[s.Pid] = s.Parent;
            foreach (var pid in new List<int>(known.Keys))
                if (!parent.ContainsKey(pid)) { known.Remove(pid); reasons.Remove(pid); }

            foreach (var s in snap)
            {
                if (s.Pid <= 4 || s.Pid == self || known.ContainsKey(s.Pid)) continue;
                if (!InScope(s.Pid, parent)) { known[s.Pid] = false; continue; }
                if (Array.IndexOf(ExcludeImages, s.Exe.ToLowerInvariant()) >= 0) { known[s.Pid] = false; continue; }

                IntPtr h = Native.OpenProcess(PROCESS_ALL_WE_NEED, false, s.Pid);
                if (h == IntPtr.Zero) { known[s.Pid] = false; continue; }
                try
                {
                    string reason = null;
                    bool agentParent;
                    if (!DryRun && InAnyJob(h)) reason = "inherited";
                    else if (known.TryGetValue(s.Parent, out agentParent) && agentParent) reason = "child of agent pid " + s.Parent;
                    else
                    {
                        string img = ImagePath(h);
                        string r = UnderRoot(img);
                        if (r != null) reason = "image under " + r;
                        else
                        {
                            string cwd, env;
                            if (ReadPeb(h, out cwd, out env))
                            {
                                r = UnderRoot(cwd);
                                if (r != null) reason = "cwd under " + r;
                                else foreach (var m in Markers) if (HasMarker(env, m)) { reason = "env " + m; break; }
                            }
                        }
                    }
                    known[s.Pid] = reason != null;
                    if (reason == null) continue;
                    reasons[s.Pid] = reason;
                    found.Add(new Member { Pid = s.Pid, Name = s.Exe, Reason = reason });
                    if (DryRun || reason == "inherited") continue;
                    Enrol(h, s);
                }
                finally { Native.CloseHandle(h); }
            }
            return found;
        }

        // A process already in a foreign job can join only an empty job, which then nests under that
        // foreign job, or one already in its hierarchy. A job-free process could join a nested job too,
        // and would then die with the foreign job's owner: the runner must never end up under Cursor's
        // job. So job-free processes go only to rooted jobs, and jobbed ones only to nested jobs.
        bool Place(IntPtr h)
        {
            bool inJob;
            bool jobless = Native.IsProcessInJob(h, IntPtr.Zero, out inJob) && !inJob;
            foreach (var s in slots)
                if (s.Rooted == jobless && Native.AssignProcessToJobObject(s.Handle, h)) return true;
            if (slots.Count >= MaxJobs) return false;
            Slot n = AddJob(jobless);
            if (Native.AssignProcessToJobObject(n.Handle, h))
            {
                if (jobless) Note("opened " + n.Name + " for job-free processes");
                return true;
            }
            Drop(n);
            return false;
        }

        public int JobIndexOf(int pid)
        {
            int found = -1;
            WithProcess(pid, PROCESS_QUERY_LIMITED, h =>
            {
                for (int i = 0; i < slots.Count; i++) { bool r; if (Native.IsProcessInJob(h, slots[i].Handle, out r) && r) { found = i; break; } }
            });
            return found;
        }

        public bool IsRooted(int i) { return slots[i].Rooted; }

        void Enrol(IntPtr h, Seen s)
        {
            bool placed = Place(h);
            int err = Marshal.GetLastWin32Error();
            uint code;
            // A rustc -vV probe can exit between the scan and the assignment, which fails with error 5.
            if (!placed && Native.GetExitCodeProcess(h, out code) && code != 259) return;
            Lower(h);
            Assigned++;
            Note((placed ? "enrolled" : "LOWERED ONLY (no job would take it, error " + err + ")") + " pid " + s.Pid + " " + s.Exe + ": " + reasons[s.Pid]);
            // The kernel lets a process join above the ceiling and only refuses its next allocation.
            if (placed && ProcessMemoryLimit != 0 && Commit(h) > ProcessMemoryLimit)
            {
                Events.Add("over-on-arrival:" + s.Pid);
                Kill(s.Pid, "was already past the per-process ceiling of " + Mb(ProcessMemoryLimit) + " when found");
            }
        }

        public bool Assign(int pid)
        {
            IntPtr h = Native.OpenProcess(PROCESS_ALL_WE_NEED, false, pid);
            if (h == IntPtr.Zero) return false;
            try { bool ok = Place(h); if (ok) Lower(h); return ok; }
            finally { Native.CloseHandle(h); }
        }

        // For measurement only: re-assert a priority class on every member, against anything else resetting it.
        public void HoldPriority(uint cls)
        {
            var t = new System.Threading.Thread(() =>
            {
                while (true)
                {
                    foreach (int pid in JobPids(slots[0].Handle)) WithProcess(pid, 0x200 | 0x1000, h => { if (Native.GetPriorityClass(h) != cls) Native.SetPriorityClass(h, cls); });
                    System.Threading.Thread.Sleep(100);
                }
            });
            t.IsBackground = true;
            t.Start();
        }

        public List<Member> Members()
        {
            var list = new List<Member>();
            var names = new Dictionary<int, string>();
            foreach (var s in Snapshot()) names[s.Pid] = s.Exe;
            foreach (var s in slots)
                foreach (int pid in JobPids(s.Handle))
                {
                    var m = new Member { Pid = pid };
                    string n; m.Name = names.TryGetValue(pid, out n) ? n : "?";
                    string r; m.Reason = reasons.TryGetValue(pid, out r) ? r : "inherited";
                    WithProcess(pid, PROCESS_QUERY_LIMITED, h => { m.Commit = Commit(h); m.PriorityClass = Native.GetPriorityClass(h); m.IoPriority = GetIoPriority(h); m.PagePriority = GetPagePriority(h); });
                    list.Add(m);
                }
            return list;
        }

        public void Lift()
        {
            foreach (var s in slots) LiftJob(s.Handle);
        }

        static void LiftJob(IntPtr j)
        {
            var x = new ExtendedLimit();
            SetInfo(j, 9, x);
            SetInfo(j, 15, new CpuRate());
            foreach (int pid in JobPids(j)) WithProcess(pid, 0x200 | 0x400 | 0x1000, Restore);
        }

        public static int LiftByName(string name)
        {
            IntPtr j = Native.OpenJobObject(0x4 | 0x10, false, name);
            if (j == IntPtr.Zero) return -1;
            try { int n = JobPids(j).Length; LiftJob(j); return n; } finally { Native.CloseHandle(j); }
        }

        public void Dispose()
        {
            foreach (var s in slots) Native.CloseHandle(s.Handle);
            slots.Clear();
            if (port != IntPtr.Zero) { Native.CloseHandle(port); port = IntPtr.Zero; }
        }
    }
}
'@

if (-not ('OpenRageLimits.Governor' -as [type])) { Add-Type -TypeDefinition $source -Language CSharp }

function Get-RepoRoot { Split-Path -Parent $PSScriptRoot }

# The checkout is the first worktree git lists, so a lane and the checkout agree on both paths below.
function Get-Checkout {
    $main = git -C (Get-RepoRoot) worktree list --porcelain 2>$null | Select-Object -First 1
    if ($main -match '^worktree (.+)$') { $Matches[1] -replace '/', '\' } else { Get-RepoRoot }
}

function Get-DefaultLaneRoot { Join-Path (Split-Path -Parent (Get-Checkout)) 'openrage-lanes' }

function Get-DefaultStateDir { Join-Path (Get-Checkout) 'target\agent-limits' }

function Write-Utf8([string]$Path, [string]$Text) {
    [System.IO.File]::WriteAllText($Path, $Text, (New-Object System.Text.UTF8Encoding($false)))
}

function Add-Utf8([string]$Path, [string[]]$Lines) {
    if ($Lines.Count -eq 0) { return }
    if ((Test-Path -LiteralPath $Path) -and (Get-Item -LiteralPath $Path).Length -gt 2MB) {
        Move-Item -LiteralPath $Path -Destination "$Path.1" -Force
    }
    [System.IO.File]::AppendAllText($Path, (($Lines -join "`r`n") + "`r`n"), (New-Object System.Text.UTF8Encoding($false)))
}

function Resolve-Ceilings {
    $installed = [OpenRageLimits.Governor]::InstalledBytes()
    if ($installed -eq 0) { $installed = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory }
    $proc = if ($ProcessMemoryGiB -gt 0) { [uint64]($ProcessMemoryGiB * 1GB) } else { [uint64]($installed / 4) }
    $job = if ($JobMemoryGiB -gt 0) { [uint64]($JobMemoryGiB * 1GB) } else { [uint64]($installed / 2) }
    [pscustomobject]@{ Installed = [uint64]$installed; Process = $proc; Job = $job }
}

function New-Governor([string]$JobName, [switch]$Dry) {
    $g = New-Object OpenRageLimits.Governor
    $lr = if ($LaneRoot) { $LaneRoot } else { Get-DefaultLaneRoot }
    $roots = @($lr) + $ExtraRoots | Where-Object { $_ } | ForEach-Object { [System.IO.Path]::GetFullPath($_).TrimEnd('\') + '\' }
    $g.Roots = [string[]]$roots
    $g.Markers = [string[]]$Markers
    # Never the editor, the IDE or the shell hosting it: they carry the marker too, and would drag the desktop into the job.
    $g.ExcludeImages = [string[]]@('cursor.exe', 'code.exe', 'explorer.exe', 'windowsterminal.exe', 'openconsole.exe', 'conhost.exe', 'rust-analyzer.exe')
    $c = Resolve-Ceilings
    $g.ProcessMemoryLimit = $c.Process
    $g.JobMemoryLimit = $c.Job
    $g.CpuRatePercent = [uint32]$CpuCapPercent
    $g.PriorityClass = switch ($Priority) { 'Idle' { 0x40 } 'BelowNormal' { 0x4000 } 'Normal' { 0x20 } }
    $g.JobName = $JobName
    $g.DryRun = [bool]$Dry
    $g
}

function Format-GiB([uint64]$b) { '{0:N1} GiB' -f ($b / 1GB) }

function Describe-Limits($g, $c) {
    "priority $Priority, I/O very low, page priority very low; per-process ceiling $(Format-GiB $g.ProcessMemoryLimit), all-agent ceiling $(Format-GiB $g.JobMemoryLimit) of $(Format-GiB $c.Installed) installed; CPU cap per tree $(if ($g.CpuRatePercent) { "$($g.CpuRatePercent)%" } else { 'none' }); roots: $($g.Roots -join ', '); markers: $($g.Markers -join ', ')"
}

# --- modes ------------------------------------------------------------------------------------

if ($DryRun) {
    $g = New-Governor -Dry
    $found = $g.Scan()
    Write-Host "Would enrol $($found.Count) process(es) now:" -ForegroundColor Cyan
    foreach ($m in $found) {
        $commit = 0
        $h = [OpenRageLimits.Native]::OpenProcess(0x1000, $false, $m.Pid)
        if ($h -ne [IntPtr]::Zero) { $commit = [OpenRageLimits.Governor]::Commit($h); [void][OpenRageLimits.Native]::CloseHandle($h) }
        '{0,7}  {1,-28} {2,8:N0} MiB  {3}' -f $m.Pid, $m.Name, ($commit / 1MB), $m.Reason
    }
    exit 0
}

if ($Run) {
    $dir = if ($StateDir) { $StateDir } else { Get-DefaultStateDir }
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $logPath = Join-Path $dir 'limits.log'
    $statePath = Join-Path $dir 'state.json'

    $created = $false
    $mutex = New-Object System.Threading.Mutex($false, $InstanceName, [ref]$created)
    if (-not $mutex.WaitOne(0)) { Add-Utf8 $logPath @("$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')  another watcher holds $InstanceName; exiting"); exit 0 }
    $stopEvent = New-Object System.Threading.EventWaitHandle($false, [System.Threading.EventResetMode]::ManualReset, $StopName)
    [void]$stopEvent.Reset()

    $g = New-Governor -JobName "Local\OpenRage.AgentLimits.$PID"
    $c = Resolve-Ceilings
    $g.Open()
    Add-Utf8 $logPath @("$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')  watcher $PID started: $(Describe-Limits $g $c)")
    $started = Get-Date
    $lastState = [DateTime]::MinValue
    $writeState = {
        param($running)
        $members = @($g.Members() | ForEach-Object { [pscustomobject][ordered]@{ pid = $_.Pid; name = $_.Name; commit_mib = [math]::Round($_.Commit / 1MB); priority = $_.PriorityClass; io = $_.IoPriority; page = $_.PagePriority; reason = $_.Reason } })
        $state = [ordered]@{
            watcher_pid = $PID; running = $running; started = $started.ToString('o'); updated = (Get-Date).ToString('o')
            limits = (Describe-Limits $g $c); jobs = $g.JobNames; enrolled_total = $g.Assigned; killed_total = $g.Killed
            commit_mib = [math]::Round((($members | Measure-Object -Property commit_mib -Sum).Sum)); members = $members
        }
        Write-Utf8 $statePath ($state | ConvertTo-Json -Depth 4)
    }
    $capPath = Join-Path $dir 'cap'
    $capStamp = [DateTime]::MinValue
    try {
        while (-not $stopEvent.WaitOne(0)) {
            # One bad iteration is logged and survived: a watcher that dies leaves agent work unlimited.
            try {
                $g.Tick($IntervalMs)
                $stamp = if (Test-Path -LiteralPath $capPath) { (Get-Item -LiteralPath $capPath).LastWriteTimeUtc } else { [DateTime]::MinValue }
                if ($stamp -ne $capStamp) {
                    $capStamp = $stamp
                    $want = if ($stamp -eq [DateTime]::MinValue) { [uint32]$CpuCapPercent } else { [uint32]((Get-Content -LiteralPath $capPath -Raw).Trim()) }
                    if ($want -ne $g.CpuRatePercent) {
                        $g.CpuRatePercent = $want
                        $g.Reapply()
                        Add-Utf8 $logPath @("$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')  CPU cap now $(if ($want) { "$want% of the machine for each agent command's tree" } else { 'off' })")
                    }
                }
                Add-Utf8 $logPath $g.DrainLog()
                if (((Get-Date) - $lastState).TotalSeconds -ge 5) { & $writeState $true; $lastState = Get-Date }
            }
            catch {
                Add-Utf8 $logPath @("$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')  ERROR $($_.Exception.Message) at $($_.InvocationInfo.PositionMessage -replace '\s+', ' ')")
                Start-Sleep -Seconds 1
            }
        }
    }
    finally {
        $g.Lift()
        Add-Utf8 $logPath ($g.DrainLog() + @("$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')  watcher $PID stopped; limits lifted and members restored to Normal"))
        & $writeState $false
        $g.Dispose()
        $mutex.ReleaseMutex()
    }
    exit 0
}

function Test-WatcherRunning {
    $m = $null
    if (-not [System.Threading.Mutex]::TryOpenExisting($InstanceName, [ref]$m)) { return $false }
    $m.Dispose(); return $true
}

if ($Stop) {
    $dir = if ($StateDir) { $StateDir } else { Get-DefaultStateDir }
    $statePath = Join-Path $dir 'state.json'
    $ev = $null
    if ([System.Threading.EventWaitHandle]::TryOpenExisting($StopName, [ref]$ev)) { [void]$ev.Set() }
    $deadline = (Get-Date).AddSeconds(15)
    while ((Test-WatcherRunning) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 250 }
    if (Test-WatcherRunning) {
        Write-Host "The watcher did not stop within 15 s; ending the scheduled task." -ForegroundColor Yellow
        Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    }
    # A watcher ended hard leaves its jobs alive around their members, still limited; lift them by name.
    if (Test-Path -LiteralPath $statePath) {
        $st = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
        foreach ($name in $st.jobs) {
            $n = [OpenRageLimits.Governor]::LiftByName($name)
            if ($n -ge 0) { Write-Host "lifted $name ($n member(s) restored to Normal)" }
        }
    }
    Write-Host "Agent limits are off. Agent processes run at full speed until ``just limits-start``." -ForegroundColor Cyan
    exit 0
}

if ($Start) {
    if (-not (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue)) {
        Write-Host "The '$TaskName' task is not registered. Run ``just limits-install`` from the checkout first." -ForegroundColor Red
        exit 1
    }
    Start-ScheduledTask -TaskName $TaskName
    $deadline = (Get-Date).AddSeconds(20)
    while (-not (Test-WatcherRunning) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 250 }
    if (Test-WatcherRunning) { Write-Host "Agent limits are on." -ForegroundColor Cyan; exit 0 }
    Write-Host "The task started but no watcher appeared within 20 s; see ``just limits-status``." -ForegroundColor Red
    exit 1
}

if ($Install) {
    $script = Join-Path $PSScriptRoot 'agent_limits.ps1'
    $taskArgs = "--headless powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File `"$script`" -Run"
    if ($ExtraRoots.Count -gt 0) { $taskArgs += " -ExtraRoots " + (($ExtraRoots | ForEach-Object { "`"$_`"" }) -join ',') }
    if ($CpuCapPercent -gt 0) { $taskArgs += " -CpuCapPercent $CpuCapPercent" }
    if ($ProcessMemoryGiB -gt 0) { $taskArgs += " -ProcessMemoryGiB $ProcessMemoryGiB" }
    if ($JobMemoryGiB -gt 0) { $taskArgs += " -JobMemoryGiB $JobMemoryGiB" }
    $user = "$env:USERDOMAIN\$env:USERNAME"
    $action = New-ScheduledTaskAction -Execute 'conhost.exe' -Argument $taskArgs -WorkingDirectory (Get-RepoRoot)
    # No trigger: the task runs only when `just limits-start` starts it, so limits are off after every restart.
    # Task priority 5 is Normal; the default 7 would start the watcher itself at BelowNormal.
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -Priority 5 -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
    $principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
    Register-ScheduledTask -TaskName $TaskName -Action $action -Settings $settings -Principal $principal -Description "Caps processes OpenRage agents launch: Idle priority and a memory ceiling. Start with: just limits-start. Remove with: just limits-uninstall" -Force | Out-Null
    Write-Host "Registered '$TaskName' for $user, on demand: conhost.exe $taskArgs" -ForegroundColor Cyan
    Write-Host "Limits stay off until ``just limits-start``, and again after every restart." -ForegroundColor Cyan
    exit 0
}

if ($Uninstall) {
    & $PSCommandPath -Stop
    if (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue) {
        Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
        Write-Host "Unregistered '$TaskName'." -ForegroundColor Cyan
    }
    exit 0
}

if ($Exec) {
    # Put this shell in a fresh job, then run the command as its child: the child is born inside, with no race.
    $g = New-Governor -JobName $null
    $g.Open()
    if (-not $g.Assign($PID)) { throw "could not place this shell in a job: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    if ($Priority -eq 'Normal') { $g.HoldPriority([OpenRageLimits.Governor]::NORMAL) }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    & ([scriptblock]::Create($Command))
    $code = $LASTEXITCODE
    $sw.Stop()
    $x = $g.Limits(0); $a = $g.Account(0)
    Write-Host ("exec: {0:N1} s wall, {1:N1} s CPU, peak job commit {2}, peak process commit {3}, priority {4}, CPU cap {5}, exit {6}" -f $sw.Elapsed.TotalSeconds, (($a.TotalUserTime + $a.TotalKernelTime) / 1e7), (Format-GiB $x.PeakJobMemoryUsed.ToUInt64()), (Format-GiB $x.PeakProcessMemoryUsed.ToUInt64()), $Priority, $(if ($CpuCapPercent) { "$CpuCapPercent%" } else { 'none' }), $code) -ForegroundColor Cyan
    exit $code
}

if ($PSCmdlet.ParameterSetName -eq 'Cap') {
    if ($SetCap -lt 0 -or $SetCap -gt 100) { Write-Host "a cap is a percentage of the whole machine, 0 (off) to 100" -ForegroundColor Red; exit 1 }
    $dir = if ($StateDir) { $StateDir } else { Get-DefaultStateDir }
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $capPath = Join-Path $dir 'cap'
    if ($SetCap -eq 0) { Remove-Item -LiteralPath $capPath -ErrorAction SilentlyContinue } else { Write-Utf8 $capPath "$SetCap" }
    Write-Host ("CPU cap {0}; a running watcher applies it within a second." -f $(if ($SetCap) { "set to $SetCap% of the machine for each agent command's process tree" } else { 'removed' })) -ForegroundColor Cyan
    if (-not (Test-WatcherRunning)) { Write-Host "No watcher is running, so nothing is capped until one starts." -ForegroundColor Yellow }
    exit 0
}

if ($SelfTest) {
    & (Join-Path $PSScriptRoot 'agent_limits_selftest.ps1') -ScratchRoot $ScratchRoot
    exit $LASTEXITCODE
}

# Status, the default.
$dir = if ($StateDir) { $StateDir } else { Get-DefaultStateDir }
$statePath = Join-Path $dir 'state.json'
$logPath = Join-Path $dir 'limits.log'
$task = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
$running = Test-WatcherRunning
Write-Host ("watcher: {0}; scheduled task: {1}" -f $(if ($running) { 'running' } else { 'NOT running' }), $(if ($task) { "$($task.State), $(if ($task.Triggers) { 'at logon (re-run just limits-install to make it on demand)' } else { 'on demand' })" } else { 'not registered' })) -ForegroundColor $(if ($running) { 'Cyan' } else { 'Yellow' })
if (Test-Path -LiteralPath $statePath) {
    $st = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
    Write-Host "limits: $($st.limits)"
    Write-Host ("as of {0}: {1} member(s), {2:N0} MiB committed; {3} enrolled and {4} killed since {5}" -f $st.updated, @($st.members).Count, $st.commit_mib, $st.enrolled_total, $st.killed_total, $st.started)
    $st.members | Sort-Object commit_mib -Descending | Select-Object -First 15 | ForEach-Object { '{0,7}  {1,-28} {2,7:N0} MiB  prio {3}  io {4}  page {5}  {6}' -f $_.pid, $_.name, $_.commit_mib, $_.priority, $_.io, $_.page, $_.reason }
}
else { Write-Host "no state at $statePath yet" }
if (Test-Path -LiteralPath $logPath) { Write-Host "--- last lines of $logPath"; Get-Content -LiteralPath $logPath -Tail 8 }
exit 0
