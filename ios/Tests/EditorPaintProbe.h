#import <Foundation/Foundation.h>
#import <UIKit/UIKit.h>

NS_ASSUME_NONNULL_BEGIN

@interface PREPEditorPaintProbe : NSObject
- (instancetype)initWithLayoutManager:(NSLayoutManager *)layoutManager;
@property(nonatomic, weak, readonly, nullable) NSLayoutManager *layoutManager;
@property(nonatomic, copy, nullable) void (^onDraw)(NSRange glyphRange, NSTimeInterval duration);
- (void)close;
@end

NS_ASSUME_NONNULL_END
