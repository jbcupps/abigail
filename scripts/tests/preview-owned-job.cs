// Test harness only. Compile-only tests never call Start or any native API.
// https://learn.microsoft.com/windows/win32/procthread/job-objects
// https://learn.microsoft.com/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

namespace Abigail.Acceptance
{
    public sealed class OwnedStableJob : IDisposable
    {
        const uint CREATE_SUSPENDED = 0x4;
        const uint CREATE_UNICODE_ENVIRONMENT = 0x400;
        const uint EXTENDED_STARTUPINFO_PRESENT = 0x80000;
        const uint CREATE_NO_WINDOW = 0x08000000;
        const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000;
        const uint WAIT_OBJECT_0 = 0;
        const uint WAIT_TIMEOUT = 258;
        IntPtr job;
        IntPtr process;
        IntPtr thread;
        bool stopped;
        public uint Pid { get; private set; }

        private OwnedStableJob() { }

        // These two helpers are pure, so tests can check quoting/environment
        // without creating a Job Object, process or credential.
        public static string QuoteArgument(string value)
        {
            if (value == null || value.IndexOf('\0') >= 0)
                throw new ArgumentException("Invalid command argument.");
            var result = new StringBuilder("\"");
            int slashes = 0;
            foreach (char c in value)
            {
                if (c == '\\') { slashes++; continue; }
                if (c == '"') result.Append('\\', slashes * 2 + 1);
                else result.Append('\\', slashes);
                result.Append(c);
                slashes = 0;
            }
            result.Append('\\', slashes * 2).Append('"');
            return result.ToString();
        }

        public static string BuildEnvironment(IDictionary<string, string> environment)
        {
            var ordered = new SortedDictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            foreach (var entry in environment)
            {
                if (String.IsNullOrWhiteSpace(entry.Key) || entry.Key.IndexOfAny(new[] {'=', '\0'}) >= 0 ||
                    entry.Value == null || entry.Value.IndexOf('\0') >= 0 || ordered.ContainsKey(entry.Key))
                    throw new ArgumentException("Invalid environment entry.");
                ordered.Add(entry.Key, entry.Value);
            }
            var block = new StringBuilder();
            foreach (var entry in ordered) block.Append(entry.Key).Append('=').Append(entry.Value).Append('\0');
            block.Append('\0');
            if (ordered.Count == 0) block.Append('\0');
            if (block.Length > 32767) throw new ArgumentException("Environment exceeds fixture limit.");
            return block.ToString();
        }

        public static OwnedStableJob Start(string executable, string[] arguments, string cwd,
            IDictionary<string, string> environment)
        {
            // Caller validates the exact installed path, all paths, environment
            // allowlist and explicit hosted-runner approval before reaching here.
            string block = BuildEnvironment(environment);
            var command = new StringBuilder(QuoteArgument(executable));
            foreach (string argument in arguments) command.Append(' ').Append(QuoteArgument(argument));
            if (command.Length >= 32767) throw new ArgumentException("Command exceeds fixture limit.");
            var owner = new OwnedStableJob();
            IntPtr nullHandle = IntPtr.Zero, attributes = IntPtr.Zero;
            IntPtr inherited = IntPtr.Zero, environmentPtr = IntPtr.Zero;
            bool attributeInitialized = false, assigned = false;
            try
            {
                owner.job = CreateJobObjectW(IntPtr.Zero, null);
                Check(owner.job != IntPtr.Zero, "Create private job");
                var limits = new JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                Check(SetInformationJobObject(owner.job, 9, ref limits,
                    (uint)Marshal.SizeOf<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()), "Set private job limit");

                var security = new SECURITY_ATTRIBUTES {
                    Length = Marshal.SizeOf<SECURITY_ATTRIBUTES>(), InheritHandle = true
                };
                nullHandle = CreateFileW("NUL", 0xC0000000, 3, ref security, 3, 0x80, IntPtr.Zero);
                Check(nullHandle != new IntPtr(-1), "Open private null stream");
                IntPtr size = IntPtr.Zero;
                InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref size);
                Check(size != IntPtr.Zero, "Size inherited handle list");
                attributes = Marshal.AllocHGlobal(size);
                Check(InitializeProcThreadAttributeList(attributes, 1, 0, ref size), "Initialize handle list");
                attributeInitialized = true;
                inherited = Marshal.AllocHGlobal(IntPtr.Size);
                Marshal.WriteIntPtr(inherited, nullHandle);
                // Only NUL is inherited. In particular the wrapper's protocol
                // pipes and private job handle never enter the child process.
                Check(UpdateProcThreadAttribute(attributes, 0, new IntPtr(0x20002), inherited,
                    new IntPtr(IntPtr.Size), IntPtr.Zero, IntPtr.Zero), "Set inherited handle list");
                var startup = new STARTUPINFOEX();
                startup.StartupInfo.cb = (uint)Marshal.SizeOf<STARTUPINFOEX>();
                startup.StartupInfo.dwFlags = 0x100; // STARTF_USESTDHANDLES
                startup.StartupInfo.hStdInput = nullHandle;
                startup.StartupInfo.hStdOutput = nullHandle;
                startup.StartupInfo.hStdError = nullHandle;
                startup.AttributeList = attributes;
                environmentPtr = Marshal.StringToHGlobalUni(block);
                PROCESS_INFORMATION created;
                Check(CreateProcessW(executable, command, IntPtr.Zero, IntPtr.Zero, true,
                    CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT,
                    environmentPtr, cwd, ref startup, out created), "Create suspended owned process");
                owner.process = created.Process;
                owner.thread = created.Thread;
                owner.Pid = created.ProcessId;
                Check(AssignProcessToJobObject(owner.job, owner.process), "Assign suspended process to private job");
                assigned = true;
                Check(ResumeThread(owner.thread) != UInt32.MaxValue, "Resume owned process");
                CloseHandle(owner.thread);
                owner.thread = IntPtr.Zero;
                return owner;
            }
            catch
            {
                // Assignment failure leaves our freshly created process
                // suspended. Terminate only that retained handle; never look
                // up/reopen a PID or identify another process by name.
                if (!assigned && owner.process != IntPtr.Zero)
                {
                    if (!TerminateProcess(owner.process, 1))
                        throw new Win32Exception(Marshal.GetLastWin32Error(), "Unassigned suspended-process cleanup failed.");
                    if (WaitForSingleObject(owner.process, 30000) != WAIT_OBJECT_0)
                        throw new InvalidOperationException("Unassigned suspended-process cleanup is uncertain.");
                }
                throw;
            }
            finally
            {
                if (environmentPtr != IntPtr.Zero) Marshal.FreeHGlobal(environmentPtr);
                if (attributeInitialized) DeleteProcThreadAttributeList(attributes);
                if (attributes != IntPtr.Zero) Marshal.FreeHGlobal(attributes);
                if (inherited != IntPtr.Zero) Marshal.FreeHGlobal(inherited);
                if (nullHandle != IntPtr.Zero && nullHandle != new IntPtr(-1)) CloseHandle(nullHandle);
                if (!assigned || owner.thread != IntPtr.Zero) owner.Dispose();
            }
        }

        public bool HasExited
        {
            get
            {
                uint result = WaitForSingleObject(process, 0);
                if (result == WAIT_OBJECT_0) return true;
                if (result == WAIT_TIMEOUT) return false;
                throw new Win32Exception(Marshal.GetLastWin32Error(), "Read owned process state failed.");
            }
        }

        public uint Stop(int timeoutMilliseconds)
        {
            if (stopped) throw new InvalidOperationException("Owned job already stopped.");
            if (timeoutMilliseconds < 1 || timeoutMilliseconds > 30000)
                throw new ArgumentException("Invalid owned job stop timeout.");
            Check(TerminateJobObject(job, 0), "Terminate private owned job");
            var watch = Stopwatch.StartNew();
            while (true)
            {
                JOBOBJECT_BASIC_ACCOUNTING_INFORMATION accounting;
                Check(QueryInformationJobObject(job, 1, out accounting,
                    (uint)Marshal.SizeOf<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>(), IntPtr.Zero),
                    "Read private job accounting");
                if (accounting.ActiveProcesses == 0 && HasExited)
                {
                    uint exitCode;
                    Check(GetExitCodeProcess(process, out exitCode), "Read owned process exit");
                    stopped = true;
                    return exitCode;
                }
                if (watch.ElapsedMilliseconds >= timeoutMilliseconds)
                    throw new TimeoutException("Private job cleanup did not confirm zero active processes.");
                Thread.Sleep(50);
            }
        }

        public void Dispose()
        {
            // KILL_ON_JOB_CLOSE also covers EOF, wrapper termination and failed
            // readiness. A stopped event is emitted only after Stop confirms
            // active-zero and the retained main process has exited.
            if (job != IntPtr.Zero) { CloseHandle(job); job = IntPtr.Zero; }
            if (thread != IntPtr.Zero) { CloseHandle(thread); thread = IntPtr.Zero; }
            if (process != IntPtr.Zero) { CloseHandle(process); process = IntPtr.Zero; }
        }

        static void Check(bool success, string operation)
        {
            if (!success) throw new Win32Exception(Marshal.GetLastWin32Error(), operation + " failed.");
        }

        [StructLayout(LayoutKind.Sequential)]
        struct SECURITY_ATTRIBUTES { public int Length; public IntPtr Descriptor; [MarshalAs(UnmanagedType.Bool)] public bool InheritHandle; }
        [StructLayout(LayoutKind.Sequential)]
        struct PROCESS_INFORMATION { public IntPtr Process, Thread; public uint ProcessId, ThreadId; }
        [StructLayout(LayoutKind.Sequential)]
        struct STARTUPINFO {
            public uint cb; public IntPtr lpReserved, lpDesktop, lpTitle;
            public uint dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
            public ushort wShowWindow, cbReserved2; public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
        }
        [StructLayout(LayoutKind.Sequential)]
        struct STARTUPINFOEX { public STARTUPINFO StartupInfo; public IntPtr AttributeList; }
        [StructLayout(LayoutKind.Sequential)]
        struct JOBOBJECT_BASIC_LIMIT_INFORMATION {
            public long PerProcessUserTimeLimit, PerJobUserTimeLimit; public uint LimitFlags;
            public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize; public uint ActiveProcessLimit;
            public UIntPtr Affinity; public uint PriorityClass, SchedulingClass;
        }
        [StructLayout(LayoutKind.Sequential)]
        struct IO_COUNTERS { public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount, ReadTransferCount, WriteTransferCount, OtherTransferCount; }
        [StructLayout(LayoutKind.Sequential)]
        struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation; public IO_COUNTERS IoInfo;
            public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
        }
        [StructLayout(LayoutKind.Sequential)]
        struct JOBOBJECT_BASIC_ACCOUNTING_INFORMATION {
            public long TotalUserTime, TotalKernelTime, ThisPeriodTotalUserTime, ThisPeriodTotalKernelTime;
            public uint TotalPageFaultCount, TotalProcesses, ActiveProcesses, TotalTerminatedProcesses;
        }
        [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
        static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);
        [DllImport("kernel32.dll", SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool SetInformationJobObject(IntPtr job, int infoClass, ref JOBOBJECT_EXTENDED_LIMIT_INFORMATION info, uint size);
        [DllImport("kernel32.dll", SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool QueryInformationJobObject(IntPtr job, int infoClass, out JOBOBJECT_BASIC_ACCOUNTING_INFORMATION info, uint size, IntPtr returnedSize);
        [DllImport("kernel32.dll", SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
        [DllImport("kernel32.dll", SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool TerminateJobObject(IntPtr job, uint exitCode);
        [DllImport("kernel32.dll", SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool TerminateProcess(IntPtr process, uint exitCode);
        [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool CreateProcessW(string app, StringBuilder command, IntPtr processAttributes, IntPtr threadAttributes,
            [MarshalAs(UnmanagedType.Bool)] bool inherit, uint flags, IntPtr environment, string cwd, ref STARTUPINFOEX startup, out PROCESS_INFORMATION info);
        [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
        static extern IntPtr CreateFileW(string path, uint access, uint share, ref SECURITY_ATTRIBUTES attributes, uint disposition, uint flags, IntPtr template);
        [DllImport("kernel32.dll", SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool InitializeProcThreadAttributeList(IntPtr list, int count, int flags, ref IntPtr size);
        [DllImport("kernel32.dll", SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool UpdateProcThreadAttribute(IntPtr list, uint flags, IntPtr attribute, IntPtr value, IntPtr size, IntPtr previous, IntPtr returned);
        [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr list);
        [DllImport("kernel32.dll", SetLastError=true)] static extern uint ResumeThread(IntPtr thread);
        [DllImport("kernel32.dll", SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
        [DllImport("kernel32.dll", SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool GetExitCodeProcess(IntPtr process, out uint code);
        [DllImport("kernel32.dll", SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)] static extern bool CloseHandle(IntPtr handle);
    }
}
