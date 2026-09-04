type Data = null | boolean | number | string | Data[] | {
    [key: string]: Data;
};
export declare function extractHost(filename: string): Data;
export {};
