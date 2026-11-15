export type ExecResult = {
    code: number | null;
    signal: number | null;
    stdout: string;
    stderr: string;
};
